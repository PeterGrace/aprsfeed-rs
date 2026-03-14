//! APRS packet formatting, passcode calculation, and filtering.
//!
//! This module converts decoded [`Ax25Frame`] values into the TNC2 monitor
//! format expected by the APRS-IS (APRS2 igate) network, and implements the
//! well-known APRS-IS passcode algorithm.

use crate::ax25::Ax25Frame;

/// Compute the APRS-IS passcode for a given callsign.
///
/// The algorithm hashes the callsign (base only, no SSID, uppercase) using a
/// simple XOR-fold over 16-bit chunks, then masks off the sign bit.
///
/// # Arguments
///
/// * `callsign` - The operator's callsign, with or without SSID suffix.
///
/// # Returns
///
/// A 15-bit passcode in the range `0..=32767`.
///
/// # Examples
///
/// ```
/// use aprsfeed_rs::aprs::calculate_passcode;
/// // Well-known test vector: KA9Q → passcode commonly cited as 10020
/// let pc = calculate_passcode("KA9Q");
/// assert!(pc < 0x8000);
/// ```
pub fn calculate_passcode(callsign: &str) -> u16 {
    // Strip the SSID suffix (everything from '-' onwards), then uppercase.
    let base: String = callsign
        .split('-')
        .next()
        .unwrap_or(callsign)
        .to_uppercase();

    let bytes = base.as_bytes();
    let mut hash: u16 = 0x73e2;

    // Process byte pairs.  If the length is odd the last iteration XORs only
    // the high byte (the low byte is effectively 0, which is a no-op).
    let mut i = 0;
    while i < bytes.len() {
        hash ^= (bytes[i] as u16) << 8;
        if i + 1 < bytes.len() {
            hash ^= bytes[i + 1] as u16;
        }
        i += 2;
    }

    // Mask to 15 bits so the result is never negative as a signed value.
    hash & 0x7fff
}

/// Format an AX.25 frame as an APRS-IS TNC2 monitor string, ready to be sent
/// to an igate server.
///
/// Returns `None` when the frame should be dropped (non-UI frame, TCPIP path,
/// third-party packet, or empty information field after stripping).
///
/// # Arguments
///
/// * `frame` - The decoded AX.25 frame.
/// * `user`  - The local station callsign used as the `qAO` gate identifier.
///
/// # Returns
///
/// `Some(line)` containing the TNC2-format string (without trailing `\r\n`),
/// or `None` if the packet should be discarded.
///
/// # Examples
///
/// ```
/// use aprsfeed_rs::ax25::{Ax25Frame, Digipeater};
/// use aprsfeed_rs::aprs::format_aprs_packet;
///
/// let frame = Ax25Frame {
///     dest: "APRS".to_string(),
///     source: "N0CALL".to_string(),
///     digipeaters: vec![],
///     control: 0x03,
///     pid: 0xf0,
///     information: b"!1234.56N/12345.67W-".to_vec(),
/// };
/// let pkt = format_aprs_packet(&frame, "W1AW").unwrap();
/// assert!(pkt.starts_with("N0CALL>APRS"));
/// assert!(pkt.contains(",qAO,W1AW"));
/// ```
pub fn format_aprs_packet(frame: &Ax25Frame, user: &str) -> Option<String> {
    // -----------------------------------------------------------------------
    // Filter 1 – only UI frames (control=0x03, pid=0xF0).
    // -----------------------------------------------------------------------
    if frame.control != 0x03 || frame.pid != 0xf0 {
        return None;
    }

    // -----------------------------------------------------------------------
    // Filter 2 – drop packets that have already traversed the internet
    //            (TCPIP in the digipeater path).
    // -----------------------------------------------------------------------
    if frame.digipeaters.iter().any(|d| d.name == "TCPIP") {
        return None;
    }

    // -----------------------------------------------------------------------
    // Filter 3 – strip parity/control characters from the information field.
    //   • Mask to 7 bits (strip parity).
    //   • Remove CR, LF, and NUL bytes.
    // -----------------------------------------------------------------------
    let filtered: Vec<u8> = frame
        .information
        .iter()
        .map(|&b| b & 0x7f)
        .filter(|&c| c != b'\r' && c != b'\n' && c != 0)
        .collect();

    if filtered.is_empty() {
        return None;
    }

    // -----------------------------------------------------------------------
    // Filter 4 – drop third-party packets (first raw byte is '{').
    //   The check uses the original (unstripped) first byte per the C source.
    // -----------------------------------------------------------------------
    if frame.information.first().copied() == Some(b'{') {
        return None;
    }

    // -----------------------------------------------------------------------
    // Build TNC2 monitor string.
    //
    // Format:  SOURCE>DEST[,DIGI[*]]*,qAO,USER:INFO
    // -----------------------------------------------------------------------
    // Pre-calculate capacity to avoid repeated reallocations.
    let digi_cap: usize = frame
        .digipeaters
        .iter()
        .map(|d| 1 + d.name.len() + if d.has_been_repeated { 1 } else { 0 })
        .sum();
    let cap = frame.source.len()
        + 1  // '>'
        + frame.dest.len()
        + digi_cap
        + 5  // ",qAO,"
        + user.len()
        + 1  // ':'
        + filtered.len();

    let mut out = String::with_capacity(cap);
    out.push_str(&frame.source);
    out.push('>');
    out.push_str(&frame.dest);

    for digi in &frame.digipeaters {
        out.push(',');
        out.push_str(&digi.name);
        if digi.has_been_repeated {
            out.push('*');
        }
    }

    out.push_str(",qAO,");
    out.push_str(user);
    out.push(':');

    // Append information bytes as lossy UTF-8.
    out.push_str(&String::from_utf8_lossy(&filtered));

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ax25::Digipeater;

    fn ui_frame(info: &[u8]) -> Ax25Frame {
        Ax25Frame {
            dest: "APRS".to_string(),
            source: "N0CALL".to_string(),
            digipeaters: vec![],
            control: 0x03,
            pid: 0xf0,
            information: info.to_vec(),
        }
    }

    // -----------------------------------------------------------------------
    // Passcode tests
    // -----------------------------------------------------------------------

    #[test]
    fn passcode_in_range() {
        let pc = calculate_passcode("N0CALL");
        assert!(pc < 0x8000);
    }

    #[test]
    fn passcode_strips_ssid() {
        assert_eq!(calculate_passcode("N0CALL"), calculate_passcode("N0CALL-9"),);
    }

    #[test]
    fn passcode_case_insensitive() {
        assert_eq!(calculate_passcode("ka9q"), calculate_passcode("KA9Q"),);
    }

    // -----------------------------------------------------------------------
    // format_aprs_packet tests
    // -----------------------------------------------------------------------

    #[test]
    fn basic_format() {
        let f = ui_frame(b"!1234.56N/12345.67W-");
        let s = format_aprs_packet(&f, "W1AW").unwrap();
        assert_eq!(s, "N0CALL>APRS,qAO,W1AW:!1234.56N/12345.67W-");
    }

    #[test]
    fn non_ui_dropped() {
        let mut f = ui_frame(b"test");
        f.control = 0x00; // not UI
        assert!(format_aprs_packet(&f, "W1AW").is_none());
    }

    #[test]
    fn tcpip_digi_dropped() {
        let mut f = ui_frame(b"test");
        f.digipeaters = vec![Digipeater {
            name: "TCPIP".to_string(),
            has_been_repeated: false,
        }];
        assert!(format_aprs_packet(&f, "W1AW").is_none());
    }

    #[test]
    fn third_party_dropped() {
        let f = ui_frame(b"{third-party}");
        assert!(format_aprs_packet(&f, "W1AW").is_none());
    }

    #[test]
    fn empty_after_strip_dropped() {
        // Only control characters → empty after filtering.
        let f = ui_frame(b"\r\n\0");
        assert!(format_aprs_packet(&f, "W1AW").is_none());
    }

    #[test]
    fn digipeater_repeated_flag() {
        let mut f = ui_frame(b"test");
        f.digipeaters = vec![
            Digipeater {
                name: "RELAY".to_string(),
                has_been_repeated: true,
            },
            Digipeater {
                name: "WIDE2-1".to_string(),
                has_been_repeated: false,
            },
        ];
        let s = format_aprs_packet(&f, "W1AW").unwrap();
        assert!(s.contains(",RELAY*,"));
        assert!(s.contains(",WIDE2-1,"));
    }

    #[test]
    fn parity_bit_stripped() {
        // High bit set on each byte → should be masked off.
        let f = ui_frame(&[b't' | 0x80, b'e' | 0x80, b's' | 0x80, b't' | 0x80]);
        let s = format_aprs_packet(&f, "GW").unwrap();
        assert!(s.ends_with(":test"));
    }
}
