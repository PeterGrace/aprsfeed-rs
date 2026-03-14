# PCAP Integration Test

**Date:** 2026-03-14T00:02:00

## Summary

Added an end-to-end integration test that reads a real captured AX.25 multicast
packet (`aprspacket.pcap`) and drives it through the full decoding pipeline,
validating the resulting APRS-IS TNC2 string against known-good values.

## Motivation

Unit tests exercise each layer (RTP, AX.25, APRS formatting) in isolation with
synthetic data.  This test validates that the layers compose correctly on actual
over-the-air data, catching any byte-order, framing, or header-stripping bugs
that synthetic fixtures might not expose.

## Structural Changes

### `src/lib.rs` (new)

A library crate entry-point that re-exports all modules as `pub`.  This is
required for Rust integration tests (under `tests/`) to access internal
symbols.  Modules exposed: `aprs`, `ax25`, `igate`, `multicast`, `pipeline`,
`rtp`.

### `src/pipeline.rs` (new)

Extracted `process_packet` from `main.rs` into a dedicated module so it is
accessible from both the binary and from integration tests via the library
crate.

### `src/main.rs`

- Removed `mod aprs; mod ax25; mod igate; mod multicast; mod rtp;` declarations
  (now provided by the library crate).
- Added `use aprsfeed_rs::{aprs, igate, multicast, pipeline};`.
- Replaced the inline `process_packet` function with a call to
  `pipeline::process_packet`.
- Retains `mod cli;` (binary-specific argument parsing, not needed in lib).

### `Cargo.toml`

Added `[dev-dependencies]`:
```toml
pcap-file = "2"
```
`pcap-file` is a pure-Rust pcap reader; no system `libpcap` required.

### `tests/pcap_integration.rs` (new)

The integration test `pcap_roundtrip`:

1. Opens `aprspacket.pcap` (Linux SLL2 / LINKTYPE\_LINUX\_SLL2 = 276).
2. Asserts the link type matches the expected value.
3. For each captured frame: strips SLL2 → IPv4 → UDP headers, filters to
   destination port 5004, and feeds the UDP payload to `process_packet`.
4. Asserts at least one APRS packet was decoded.
5. Validates structural TNC2 format (`SOURCE>DEST,...,qAO,STATION:INFO`).
6. Asserts exact field values for the known fixture:
   - Source: `W3POG-7`
   - Destination: `T0QVLZ` (MIC-E encoded)
   - Digipeaters: `WIDE1-1`, `WIDE2-1` (neither repeated)
   - Gate path: `,qAO,N0CALL:`
   - Info field: non-empty, no raw CR/LF/NUL

## Fixture

`aprspacket.pcap` — single-packet capture of a ka9q-radio AX.25 multicast
RTP stream, captured on Linux (cooked capture v2 / SLL2 link type).

File must be present at the repository root for `cargo test` to pass.
