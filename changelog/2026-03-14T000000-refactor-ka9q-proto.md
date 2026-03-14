# Refactor: Replace local protocol modules with ka9q-proto dependency

**Date:** 2026-03-14
**Type:** Refactor
**Breaking:** No (public API preserved via re-exports)

## Summary

The five protocol modules that were duplicated between `aprsfeed-rs` and the new
shared library `ka9q-proto` have been removed from this crate.  All functionality
is now provided by `ka9q-proto`, eliminating code duplication and ensuring a
single canonical implementation across all ka9q-radio tools.

## Modules removed from aprsfeed-rs

| File removed            | Replacement                  |
|-------------------------|------------------------------|
| `src/aprs.rs`           | `ka9q_proto::aprs`           |
| `src/ax25.rs`           | `ka9q_proto::ax25`           |
| `src/igate.rs`          | `ka9q_proto::igate`          |
| `src/multicast.rs`      | `ka9q_proto::multicast`      |
| `src/rtp.rs`            | `ka9q_proto::rtp`            |

## Changes

### `Cargo.toml`
- Added `ka9q-proto = { path = "../ka9q-proto" }` dependency.
- Removed direct dependencies no longer needed by aprsfeed-rs itself:
  `libc`, `socket2`, `thiserror` (all transitively provided by ka9q-proto).

### `src/lib.rs`
- Replaced five `pub mod <name>;` declarations with `pub use ka9q_proto::<name>;`
  re-exports, preserving the `aprsfeed_rs::<module>` public API for callers.
- Sole remaining local module: `pipeline` (application-specific logic).

### `src/pipeline.rs`
- Updated import from `use crate::{aprs, ax25, rtp}` to
  `use ka9q_proto::{aprs, ax25, rtp}`.

## Behavioral notes

- The APRS-IS login banner now reads `vers ka9q-proto 0.1.0` instead of
  `vers aprsfeed-rs 0.1.1`, reflecting that the iGate connection logic
  is provided by the shared library.
- The TCPIP loop-prevention filter is now case-insensitive and prefix-matched
  (`starts_with("TCPIP")`), matching the ka9q-proto implementation.  The
  previous check was an exact case-sensitive match against `"TCPIP"`.

## Verification

```
cargo build      # zero warnings
cargo test       # pcap_roundtrip passes
cargo clippy -- -D warnings  # zero warnings
```
