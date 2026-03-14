//! `aprsfeed-rs` library crate.
//!
//! Exposes the decoding and networking modules so they can be exercised by
//! integration tests and consumed by the binary entry-point.
//!
//! Protocol modules (`aprs`, `ax25`, `multicast`, `rtp`) are re-exported
//! directly from [`ka9q_proto`], the shared protocol library.  The
//! APRS-IS iGate logic (`igate`) lives here because it is specific to this
//! application.

pub use ka9q_proto::aprs;
pub use ka9q_proto::ax25;
pub use ka9q_proto::multicast;
pub use ka9q_proto::rtp;

pub mod igate;

pub mod pipeline;
