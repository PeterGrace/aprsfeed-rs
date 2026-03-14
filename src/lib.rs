//! `aprsfeed-rs` library crate.
//!
//! Exposes the decoding and networking modules so they can be exercised by
//! integration tests and consumed by the binary entry-point.

pub mod aprs;
pub mod ax25;
pub mod igate;
pub mod multicast;
pub mod pipeline;
pub mod rtp;
