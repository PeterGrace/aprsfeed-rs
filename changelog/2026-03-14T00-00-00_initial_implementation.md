# aprsfeed-rs – Initial Implementation

**Date:** 2026-03-14T00:00:00
**Author:** Peter Grace
**Version:** 0.1.0

---

## Overview

Complete Rust reimplementation of the `ka9q-radio` `aprsfeed.c` application.
The binary receives raw AX.25 packet frames carried inside RTP datagrams on a
multicast UDP stream, decodes them, applies the same filtering rules as the
original C program, formats them as APRS-IS TNC2 monitor strings, and forwards
them to an APRS2 igate server over a persistent (auto-reconnecting) TCP
connection.

---

## Module Breakdown

### `Cargo.toml`

Replaced the skeleton dependency set with a minimal, focused set:

| Crate | Purpose |
|---|---|
| `anyhow` | Application-level error propagation with context |
| `clap` (derive) | CLI argument parsing |
| `dotenvy` | Optional `.env` file loading |
| `socket2` | Low-level socket configuration (multicast join, `SO_REUSEPORT`) |
| `thiserror` | Custom error type definitions |
| `tokio` (full async) | Async runtime, TCP/UDP, channels, timers |
| `tracing` + `tracing-subscriber` | Structured logging |

The `tokio_unstable` rustflag already present in `.cargo/config.toml` is
preserved so the Tokio task-dump / console features remain available.

---

### `src/cli.rs` – Argument Parsing

Defines the `Args` struct with `clap` derive macros, matching the flags of the
original C program:

| Flag | Default | Description |
|---|---|---|
| `-u` / `--user` | *(required)* | APRS callsign |
| `-p` / `--passcode` | auto-calculated | APRS-IS passcode |
| `-I` / `--input` | `ax25.mcast.local` | Multicast group |
| `-H` / `--host` | `noam.aprs2.net` | iGate server |
| `-P` / `--port` | `14580` | iGate TCP port |
| `-f` / `--logfile` | *(none)* | Optional log file path |
| `-v` / `--verbose` | `false` | Enable debug logging |

---

### `src/rtp.rs` – RTP Header Parsing

Implements RFC 3550 §5.1 fixed-header parsing plus optional CSRC list and
extension-header skipping.  Key constant:

- `AX25_PAYLOAD_TYPE = 96` – the payload-type value ka9q-radio uses for raw
  AX.25 frames.

`RtpHeader::parse(&[u8]) -> Option<(RtpHeader, &[u8])>` returns a zero-copy
reference to the payload slice within the original buffer.

Edge cases handled:
- CSRC list (`CC` field) skip.
- Extension header (`X` bit) with word-count skip.
- Padding (`P` bit) stripping based on final byte.

---

### `src/ax25.rs` – AX.25 Frame Parsing

Decodes the AX.25 wire format as used by ka9q-radio:

- Address-field end detection via the end-of-address bit (bit 0 of SSID byte).
- Validation: address field length must be a multiple of 7 and contain at
  least 2 entries (dest + source).
- `decode_callsign`: shifts each byte right by 1 to recover ASCII, appends
  SSID as `-N` when non-zero.
- Digipeater H-bit (has-been-repeated) extraction.
- 2-byte FCS stripped from the tail of the information field.

Public types: `Ax25Frame`, `Digipeater`.

---

### `src/aprs.rs` – APRS Formatting and Filtering

#### `calculate_passcode(callsign: &str) -> u16`

Standard APRS-IS passcode algorithm:
1. Strip SSID suffix, uppercase.
2. XOR-fold byte pairs into a `u16` starting at `0x73e2`.
3. Mask to 15 bits.

#### `format_aprs_packet(frame: &Ax25Frame, user: &str) -> Option<String>`

Filtering pipeline (matches C source behaviour):

1. Drop non-UI frames (`control != 0x03 || pid != 0xF0`).
2. Drop frames with `TCPIP` in the digipeater path (already-gated packets).
3. Strip parity bits and control characters (`CR`, `LF`, `NUL`) from the
   information field.
4. Drop frames whose information field is empty after stripping.
5. Drop third-party packets (raw first byte `== '{'`).

Output format: `SOURCE>DEST[,DIGI[*]]...,qAO,USER:INFO`

---

### `src/multicast.rs` – UDP Multicast Socket

`create_multicast_socket(mcast_addr_str, port) -> anyhow::Result<UdpSocket>`

1. Resolves the hostname asynchronously with `tokio::net::lookup_host`.
2. Creates a `socket2::Socket` with `SO_REUSEADDR` + `SO_REUSEPORT`.
3. Binds to `0.0.0.0:port` (or `[::]:port` for IPv6).
4. Joins the multicast group on the wildcard interface.
5. Sets non-blocking mode and wraps into `tokio::net::UdpSocket`.

Both IPv4 (`join_multicast_v4`) and IPv6 (`join_multicast_v6`) paths are
supported.

Constant: `DEFAULT_RTP_PORT = 5004`.

---

### `src/igate.rs` – iGate TCP Connection

`run_igate(host, port, user, passcode, rx)`:
- Outer loop: reconnect forever; waits 600 s after each failure.

`connect_and_run(...)`:
- `resolve_with_retry`: up to 10 attempts, 500 ms back-off, re-resolves DNS on
  each try (handles round-robin / DNS changes).
- Sends the APRS-IS login line: `user USER pass PASS vers aprsfeed-rs VERSION`.
- `tokio::select!` loop:
  - **Server branch**: reads lines from the server (banner, `#` keepalives),
    logs at `debug`.
  - **Channel branch**: receives formatted packets from `mpsc::Receiver<String>`,
    writes `PACKET\r\n` to the server, flushes.
  - `Receiver::recv()` returning `None` triggers a clean shutdown (`Ok(())`).
  - Server close / I/O error returns `Err(...)` → outer loop reconnects after
    the back-off delay.

---

### `src/main.rs` – Entry Point

1. `dotenvy::dotenv()` – optional `.env` loading.
2. `Args::parse()` – CLI arguments.
3. Tracing setup: if `--logfile` is given, fan-out to both stderr and the file
   via two `fmt::layer()` instances; otherwise stderr only.
4. Passcode: use `--passcode` if provided, otherwise `aprs::calculate_passcode`.
5. Multicast socket created via `multicast::create_multicast_socket`.
6. `mpsc::channel(1000)` – bounded channel with capacity 1000.
7. `tokio::spawn(igate::run_igate(...))` – iGate task runs concurrently.
8. UDP receive loop:
   - `socket.recv(&mut buf)` – single allocation, reused every iteration.
   - `process_packet(datagram, user)` – parse RTP → AX.25 → APRS string.
   - `tx.try_send(packet)` – non-blocking; logs `warn!` and drops on `Full`;
     exits on `Closed`.

`process_packet` is a pure function (no I/O) that sequences: RTP parse → AX.25
parse → APRS format.

---

## Testing

Unit tests are included in each module under `#[cfg(test)]` blocks:

- `rtp`: basic parse, too-short input, padding stripping, extension skipping.
- `ax25`: basic UI frame, digipeater H-bit, SSID suffix, edge cases.
- `aprs`: passcode range/case/SSID, format output, all five filter paths.

Run with:

```sh
cargo test
```

---

## Known Limitations / Future Work

- No support for RTP jitter-buffer reordering (out-of-order packets are
  forwarded as received; the original C code also does not reorder).
- IPv6 multicast scope ID is taken from the DNS resolution result; this may
  need manual override for link-local groups.
- No APRS-IS server-side filtering (filter string); future `-f` flag could add
  this as an extra field in the login line.
