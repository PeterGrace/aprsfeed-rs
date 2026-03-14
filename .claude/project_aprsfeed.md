---
name: aprsfeed-rs project context
description: Context on the aprsfeed-rs Rust reimplementation of ka9q-radio aprsfeed.c
type: project
---

A Rust reimplementation of ka9q-radio's `aprsfeed.c` APRS igate feeder.

**Why:** User wanted a faithful Rust port of the C application from `../ka9q-radio/src/aprsfeed.c`.

**How to apply:** When making changes, refer to the original C source in `/home/pgrace/repos/ka9q-radio/src/` for behavior reference. The Rust implementation matches the C behavior exactly.

## Architecture

```
UDP multicast recv (main) → mpsc channel(1000) → iGate TCP task
```

## Module layout
- `src/main.rs` — entry point, tracing setup, UDP recv loop
- `src/cli.rs` — clap Args struct
- `src/rtp.rs` — RTP header parser (AX25_PAYLOAD_TYPE = 96)
- `src/ax25.rs` — AX.25 frame decoder (matches ka9q-radio wire format exactly, strips 2-byte FCS)
- `src/aprs.rs` — TNC2 monitor string formatter, APRS passcode calculator, packet filters
- `src/multicast.rs` — socket2-based IPv4/IPv6 multicast UDP socket (DEFAULT_RTP_PORT = 5004)
- `src/igate.rs` — auto-reconnecting APRS-IS TCP forwarder (10-minute backoff, 10 DNS retries)

## Key behaviors
- AX.25 FCS stripping: ka9q-radio includes 2-byte FCS in RTP payload; strip with `data[..len-2]`
- Passcode: `hash=0x73e2`, XOR pairs of uppercase callsign bytes (no SSID), `& 0x7fff`
- Filters: UI frame (ctrl=0x03, pid=0xf0), non-empty info, no TCPIP digi, no '{' first byte
- Login format: `user <CALL> pass <PASSCODE> vers aprsfeed-rs <VERSION>\r\n`
- TNC2 format: `SRC>DST,DIGI1[*],...,qAO,USER:PAYLOAD`
