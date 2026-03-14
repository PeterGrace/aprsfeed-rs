# Migrate `igate` module from `ka9q-proto` into `aprsfeed-rs`

**Date:** 2026-03-14

## Summary

Moved `src/igate.rs` from `../ka9q-proto` into `aprsfeed-rs` as a local
application module.  The iGate logic is APRS-IS-specific and only consumed by
this crate; keeping it in the shared library added unnecessary coupling.

## Changes

### `aprsfeed-rs`

- Added `src/igate.rs` (moved verbatim from `ka9q-proto/src/igate.rs`).
- Updated login banner version string from `vers ka9q-proto` to
  `vers aprsfeed-rs` so APRS-IS servers see the correct software identifier.
- `src/lib.rs`: replaced `pub use ka9q_proto::igate` re-export with
  `pub mod igate` (local module declaration).  Updated module-level doc comment
  accordingly.

### `ka9q-proto`

- Removed `src/igate.rs`.
- `src/lib.rs`: removed `pub mod igate` declaration and the corresponding
  table row in the crate-level doc comment.

## Rationale

`rtp`, `ax25`, `multicast`, and `mdns` are general-purpose protocol primitives
that could plausibly be reused by other `ka9q-radio` tools (e.g. `packetd-rs`).
The iGate module, by contrast, implements the APRS-IS TCP login/forwarding
protocol — an application-layer concern specific to an iGate feeder.  Keeping
application logic in a shared library violates the single-responsibility
principle and makes `ka9q-proto` harder to reason about.

## Verification

Both crates compile and pass `cargo clippy -- -D warnings` with zero warnings.
