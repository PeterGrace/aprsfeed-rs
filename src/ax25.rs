//! AX.25 frame parsing.
//!
//! Implements the subset of AX.25 required for APRS UI-frame decoding,
//! matching the behaviour of the reference `ka9q-radio` `aprsfeed.c`
//! implementation.
//!
//! # Wire format
//!
//! ```text
//! Address field  N×7 bytes  (N ≥ 2; dest, src, 0+ digipeaters)
//!   Each address entry:
//!     bytes 0-5  callsign ASCII, each byte shifted left by 1 (bit0 always 0)
//!     byte  6    SSID byte:
//!                  bit7   H-bit (has-been-repeated for digipeaters)
//!                  bits6-5 reserved
//!                  bits4-1 SSID nibble
//!                  bit0   end-of-address flag (1 only in the last entry)
//! Control byte   1 byte   (0x03 for UI frames)
//! PID byte       1 byte   (0xF0 for no layer-3 protocol)
//! Information    variable
//! FCS            2 bytes  (included in ka9q-radio RTP payload – stripped here)
//! ```

/// A digipeater entry in the AX.25 address field.
#[derive(Debug, Clone, PartialEq)]
pub struct Digipeater {
    /// Digipeater callsign, with SSID appended as "-N" when non-zero.
    pub name: String,
    /// Set when this digipeater has already re-transmitted the frame
    /// (the H-bit in the SSID byte).
    pub has_been_repeated: bool,
}

/// A fully decoded AX.25 frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Ax25Frame {
    /// Destination callsign (with optional "-SSID" suffix).
    pub dest: String,
    /// Source callsign (with optional "-SSID" suffix).
    pub source: String,
    /// Zero or more digipeater entries following the source address.
    pub digipeaters: Vec<Digipeater>,
    /// Frame control byte.
    pub control: u8,
    /// Protocol identifier byte.
    pub pid: u8,
    /// Raw information-field bytes (FCS already stripped).
    pub information: Vec<u8>,
}

/// Decode a 7-byte AX.25 address entry into a callsign string.
///
/// Each callsign character is stored as `ASCII << 1`; we shift right by 1
/// to recover the character.  A trailing SSID of 0 is omitted.
///
/// # Arguments
///
/// * `bytes` - Exactly 7-byte slice for one address entry.
///
/// # Returns
///
/// The callsign string, e.g. `"N0CALL"` or `"N0CALL-3"`.
fn decode_callsign(bytes: &[u8]) -> String {
    debug_assert_eq!(
        bytes.len(),
        7,
        "AX.25 address entry must be exactly 7 bytes"
    );

    // Recover the 6 callsign characters by shifting right 1 bit.
    let mut callsign = String::with_capacity(9); // "XXXXXX-NN"
    for &raw in bytes.iter().take(6) {
        let c = raw >> 1;
        // A space (0x20) indicates padding – stop here.
        if c == b' ' {
            break;
        }
        callsign.push(c as char);
    }

    // SSID nibble lives in bits 4-1 of the 7th byte.
    let ssid = (bytes[6] >> 1) & 0x0f;
    if ssid != 0 {
        callsign.push('-');
        // Format the SSID digit(s) without allocating a temporary String.
        if ssid >= 10 {
            callsign.push((b'0' + ssid / 10) as char);
        }
        callsign.push((b'0' + ssid % 10) as char);
    }

    callsign
}

impl Ax25Frame {
    /// Parse a raw byte buffer into an [`Ax25Frame`].
    ///
    /// The buffer is expected to be the RTP payload as produced by
    /// `ka9q-radio`, which includes the 2-byte FCS at the end.
    ///
    /// # Arguments
    ///
    /// * `data` - Raw frame bytes including FCS.
    ///
    /// # Returns
    ///
    /// `Some(frame)` on success, `None` if the buffer is malformed or too
    /// short to contain a valid AX.25 UI frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use aprsfeed_rs::ax25::Ax25Frame;
    /// // A minimal two-address frame; callsigns are ASCII left-shifted 1.
    /// ```
    pub fn parse(data: &[u8]) -> Option<Ax25Frame> {
        // ----------------------------------------------------------------
        // Step 1 – locate the end of the address field.
        //
        // The end-of-address flag is bit0 of the SSID byte (every 7th byte
        // starting at index 6).  Walk the data looking for the first byte
        // with bit0 set; that byte is the SSID byte of the last address entry.
        // ----------------------------------------------------------------
        let ctl_offs = data
            .iter()
            .enumerate()
            .find(|&(_, &b)| b & 0x01 != 0)
            .map(|(idx, _)| idx + 1)?; // control byte is one past the end-flag byte

        // The address field must be an exact multiple of 7 bytes.
        if ctl_offs % 7 != 0 {
            return None;
        }

        let addr_count = ctl_offs / 7;
        // Need at least dest + source.
        if addr_count < 2 {
            return None;
        }

        // We need at least: address field + control + pid + 2-byte FCS.
        if data.len() < ctl_offs + 4 {
            return None;
        }

        // ----------------------------------------------------------------
        // Step 2 – decode addresses.
        // ----------------------------------------------------------------
        let dest = decode_callsign(&data[0..7]);
        let source = decode_callsign(&data[7..14]);

        // Optional digipeater entries: indices 2..(addr_count).
        let mut digipeaters = Vec::with_capacity(addr_count.saturating_sub(2));
        for i in 0..(addr_count - 2) {
            let base = 7 * (2 + i);
            let name = decode_callsign(&data[base..base + 7]);
            // H-bit is bit7 of the SSID byte (7th byte of the entry).
            let has_been_repeated = (data[base + 6] & 0x80) != 0;
            digipeaters.push(Digipeater {
                name,
                has_been_repeated,
            });
        }

        // ----------------------------------------------------------------
        // Step 3 – control, PID, and information field.
        // ----------------------------------------------------------------
        let control = data[ctl_offs];
        let pid = data[ctl_offs + 1];

        let info_start = ctl_offs + 2;
        // Strip the 2-byte FCS appended by ka9q-radio.
        if data.len() < info_start + 2 {
            return None;
        }
        let info_end = data.len() - 2;
        let information = data[info_start..info_end].to_vec();

        Some(Ax25Frame {
            dest,
            source,
            digipeaters,
            control,
            pid,
            information,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a callsign into the 7-byte AX.25 wire representation.
    /// Pass `end_flag = true` for the last address entry.
    fn encode_addr(call: &str, ssid: u8, end_flag: bool, h_bit: bool) -> Vec<u8> {
        let mut bytes = vec![0u8; 7];
        for (i, c) in call.bytes().enumerate().take(6) {
            bytes[i] = c << 1;
        }
        // Pad remaining callsign bytes with space << 1.
        for i in call.len()..6 {
            bytes[i] = b' ' << 1;
        }
        let mut ssid_byte = (ssid & 0x0f) << 1;
        if end_flag {
            ssid_byte |= 0x01;
        }
        if h_bit {
            ssid_byte |= 0x80;
        }
        bytes[6] = ssid_byte;
        bytes
    }

    fn build_frame(dest: &str, src: &str, digis: &[(&str, bool)], info: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(&encode_addr(dest, 0, false, false));
        if digis.is_empty() {
            frame.extend_from_slice(&encode_addr(src, 0, true, false));
        } else {
            frame.extend_from_slice(&encode_addr(src, 0, false, false));
            for (i, (name, h)) in digis.iter().enumerate() {
                let end = i == digis.len() - 1;
                frame.extend_from_slice(&encode_addr(name, 0, end, *h));
            }
        }
        frame.push(0x03); // control = UI
        frame.push(0xf0); // pid = no L3
        frame.extend_from_slice(info);
        frame.extend_from_slice(&[0x00, 0x00]); // FCS placeholder
        frame
    }

    #[test]
    fn basic_ui_frame() {
        let raw = build_frame("APRS", "N0CALL", &[], b"!1234.56N/12345.67W-Test");
        let frame = Ax25Frame::parse(&raw).unwrap();
        assert_eq!(frame.dest, "APRS");
        assert_eq!(frame.source, "N0CALL");
        assert!(frame.digipeaters.is_empty());
        assert_eq!(frame.control, 0x03);
        assert_eq!(frame.pid, 0xf0);
        assert_eq!(frame.information, b"!1234.56N/12345.67W-Test");
    }

    #[test]
    fn digipeater_decoded() {
        let raw = build_frame(
            "APWIDE",
            "W1AW",
            &[("RELAY", false), ("WIDE", true)],
            b":test:",
        );
        let frame = Ax25Frame::parse(&raw).unwrap();
        assert_eq!(frame.digipeaters.len(), 2);
        assert_eq!(frame.digipeaters[0].name, "RELAY");
        assert!(!frame.digipeaters[0].has_been_repeated);
        assert_eq!(frame.digipeaters[1].name, "WIDE");
        assert!(frame.digipeaters[1].has_been_repeated);
    }

    #[test]
    fn ssid_appended() {
        let mut frame_bytes = encode_addr("N0CALL", 3, false, false);
        frame_bytes.extend_from_slice(&encode_addr("DEST", 0, true, false));
        frame_bytes.extend_from_slice(&[0x03, 0xf0, b'/', 0x00, 0x00]);
        // dest is first 7 bytes → "N0CALL-3"; source is next 7 → "DEST"
        let frame = Ax25Frame::parse(&frame_bytes).unwrap();
        assert_eq!(frame.source, "DEST");
        assert_eq!(frame.dest, "N0CALL-3");
    }

    #[test]
    fn too_short_returns_none() {
        assert!(Ax25Frame::parse(&[]).is_none());
        assert!(Ax25Frame::parse(&[0x01]).is_none());
    }
}
