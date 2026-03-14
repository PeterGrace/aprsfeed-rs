//! End-to-end packet processing pipeline.
//!
//! Receives a raw UDP datagram (an RTP packet from ka9q-radio) and returns a
//! formatted APRS-IS TNC2 monitor string when the datagram contains a valid
//! AX.25 UI APRS frame, or `None` in all other cases.

use tracing::debug;

use crate::{aprs, ax25, rtp};

/// Parse one UDP datagram and return a formatted APRS-IS string if it carries
/// a valid AX.25 UI APRS frame, or `None` otherwise.
///
/// The processing steps are:
/// 1. Parse the RTP fixed header and locate the payload.
/// 2. Reject payload types other than [`rtp::AX25_PAYLOAD_TYPE`] (96).
/// 3. Decode the AX.25 frame from the RTP payload.
/// 4. Format and filter the APRS-IS TNC2 string.
///
/// # Arguments
///
/// * `data` - Raw UDP datagram bytes starting at the RTP header.
/// * `user` - Local station callsign injected into the TNC2 path as `qAO,USER`.
///
/// # Returns
///
/// `Some(tnc2_string)` when decoding succeeds and the frame passes all APRS
/// filters; `None` for any malformed, filtered, or non-APRS input.
pub fn process_packet(data: &[u8], user: &str) -> Option<String> {
    let (rtp_hdr, payload) = rtp::RtpHeader::parse(data)?;

    if rtp_hdr.payload_type != rtp::AX25_PAYLOAD_TYPE {
        debug!(
            pt = rtp_hdr.payload_type,
            "Ignoring RTP packet with unexpected payload type"
        );
        return None;
    }

    let frame = ax25::Ax25Frame::parse(payload)?;
    aprs::format_aprs_packet(&frame, user)
}
