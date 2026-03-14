//! `aprsfeed-rs` library crate.
//!
//! Exposes the decoding and networking modules so they can be exercised by
//! integration tests and consumed by the binary entry-point.
//!
//! Protocol modules (`aprs`, `ax25`, `igate`, `multicast`, `rtp`) are
//! re-exported directly from [`ka9q_proto`], the shared protocol library.
//! Only the application-specific pipeline logic lives here.

pub use ka9q_proto::aprs;
pub use ka9q_proto::ax25;
pub use ka9q_proto::igate;
pub use ka9q_proto::multicast;
pub use ka9q_proto::rtp;

pub mod pipeline;
