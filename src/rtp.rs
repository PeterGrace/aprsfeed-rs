//! RTP (Real-time Transport Protocol) header parsing.
//!
//! Only the fields required for demultiplexing AX.25 payload streams are
//! parsed.  The implementation follows RFC 3550 §5.1.

/// RTP payload-type value used by ka9q-radio for raw AX.25 frames.
pub const AX25_PAYLOAD_TYPE: u8 = 96;

/// Parsed representation of an RTP fixed header plus optional extension.
///
/// Fields map directly to the RFC 3550 §5.1 bit layout.
#[derive(Debug, Clone, PartialEq)]
pub struct RtpHeader {
    /// RTP version – must be 2 for all current streams.
    pub version: u8,
    /// Padding flag: if set the payload ends with padding bytes.
    pub pad: bool,
    /// Extension flag: if set a header extension follows the CSRC list.
    pub extension: bool,
    /// CSRC count: number of contributing-source identifiers.
    pub cc: u8,
    /// Marker bit (application-specific).
    pub marker: bool,
    /// Payload type identifier.
    pub payload_type: u8,
    /// Sequence number.
    pub seq: u16,
    /// Timestamp.
    pub timestamp: u32,
    /// Synchronisation source identifier.
    pub ssrc: u32,
}

impl RtpHeader {
    /// Parse an RTP packet, returning the decoded header and a slice of the
    /// payload bytes, or `None` if the buffer is too short or malformed.
    ///
    /// # Arguments
    ///
    /// * `data` - Raw UDP datagram bytes, starting at the beginning of the RTP
    ///   fixed header.
    ///
    /// # Returns
    ///
    /// `Some((header, payload))` on success, `None` on any parse failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use aprsfeed_rs::rtp::{RtpHeader, AX25_PAYLOAD_TYPE};
    /// let mut pkt = vec![
    ///     0x80, 0x60, 0x00, 0x01, // V=2, PT=96, seq=1
    ///     0x00, 0x00, 0x00, 0x00, // timestamp=0
    ///     0xDE, 0xAD, 0xBE, 0xEF, // ssrc
    ///     0x01, 0x02,             // payload
    /// ];
    /// let (hdr, payload) = RtpHeader::parse(&pkt).unwrap();
    /// assert_eq!(hdr.payload_type, AX25_PAYLOAD_TYPE);
    /// assert_eq!(payload, &[0x01, 0x02]);
    /// ```
    pub fn parse(data: &[u8]) -> Option<(RtpHeader, &[u8])> {
        // Minimum fixed header is 12 bytes.
        if data.len() < 12 {
            return None;
        }

        // First 32-bit word (big-endian):
        //   bits 31-30 : version (2 bits)
        //   bit  29    : padding
        //   bit  28    : extension
        //   bits 27-24 : cc (4 bits)
        //   bit  23    : marker
        //   bits 22-16 : payload type (7 bits)
        //   bits 15-0  : sequence number
        let word0 = u16::from_be_bytes([data[0], data[1]]);
        let seq = u16::from_be_bytes([data[2], data[3]]);

        let version = (data[0] >> 6) & 0x3;
        let pad = (data[0] & 0x20) != 0;
        let extension = (data[0] & 0x10) != 0;
        let cc = data[0] & 0x0f;
        let marker = (data[1] & 0x80) != 0;
        let payload_type = data[1] & 0x7f;

        // Suppress the unused-variable warning for word0; it was used for
        // documentation/clarity but all fields are extracted from individual bytes.
        let _ = word0;

        let timestamp = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ssrc = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);

        // Fixed header is 12 bytes; CSRC list adds cc×4 bytes.
        let csrc_len = (cc as usize) * 4;
        let after_fixed = 12 + csrc_len;
        if data.len() < after_fixed {
            return None;
        }

        // Optional header extension (RFC 3550 §5.3.1).
        // Layout: 16-bit profile id | 16-bit length (in 32-bit words) | length×4 bytes data.
        let after_ext = if extension {
            if data.len() < after_fixed + 4 {
                return None;
            }
            // The low 16 bits of the extension word carry the word-count.
            let ext_words =
                u16::from_be_bytes([data[after_fixed + 2], data[after_fixed + 3]]) as usize;
            after_fixed + 4 + ext_words * 4
        } else {
            after_fixed
        };

        if data.len() < after_ext {
            return None;
        }

        // Payload slice – strip trailing padding if the P flag is set.
        let payload_end = if pad {
            if data.len() == after_ext {
                // No payload bytes at all; cannot read pad count.
                return None;
            }
            let pad_count = *data.last()? as usize;
            if pad_count == 0 || data.len() < after_ext + pad_count {
                return None;
            }
            data.len() - pad_count
        } else {
            data.len()
        };

        let payload = &data[after_ext..payload_end];

        let header = RtpHeader {
            version,
            pad,
            extension,
            cc,
            marker,
            payload_type,
            seq,
            timestamp,
            ssrc,
        };

        Some((header, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rtp(pt: u8, payload: &[u8]) -> Vec<u8> {
        let mut pkt = vec![
            0x80, // V=2, P=0, X=0, CC=0
            pt,   // M=0, PT
            0x00, 0x01, // seq=1
            0x00, 0x00, 0x00, 0x00, // timestamp=0
            0x00, 0x00, 0x00, 0x01, // ssrc=1
        ];
        pkt.extend_from_slice(payload);
        pkt
    }

    #[test]
    fn parse_basic() {
        let pkt = make_rtp(AX25_PAYLOAD_TYPE, &[0xAA, 0xBB]);
        let (hdr, payload) = RtpHeader::parse(&pkt).unwrap();
        assert_eq!(hdr.version, 2);
        assert_eq!(hdr.payload_type, AX25_PAYLOAD_TYPE);
        assert_eq!(hdr.seq, 1);
        assert!(!hdr.pad);
        assert!(!hdr.extension);
        assert_eq!(payload, &[0xAA, 0xBB]);
    }

    #[test]
    fn too_short_returns_none() {
        assert!(RtpHeader::parse(&[0x80, 0x60, 0x00]).is_none());
    }

    #[test]
    fn padding_stripped() {
        // P=1 → bit5 of first byte
        let mut pkt = vec![
            0xa0,
            AX25_PAYLOAD_TYPE,
            0x00,
            0x02,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x01,
            0xDE,
            0xAD,
            0x02, // 2-byte payload + 1-byte pad-count (= 2)
        ];
        // Pad count is the last byte = 2 → strip last 2 bytes: payload = [0xDE]
        // But wait: pad_count=2 includes itself, so we strip 2 bytes from the
        // end of the full data (after_ext..len-2 would leave [0xDE]).
        // Make the final byte = 1 so only 1 byte is stripped:
        *pkt.last_mut().unwrap() = 1;
        let (_, payload) = RtpHeader::parse(&pkt).unwrap();
        assert_eq!(payload, &[0xDE, 0xAD]);
    }

    #[test]
    fn extension_skipped() {
        let pkt = vec![
            0x90,
            AX25_PAYLOAD_TYPE,
            0x00,
            0x01, // X=1
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x01,
            // Extension: profile=0xBEEF, length=1 (1 word = 4 bytes)
            0xBE,
            0xEF,
            0x00,
            0x01,
            0x00,
            0x00,
            0x00,
            0x00, // 1 extension word
            0xCC,
            0xDD, // actual payload
        ];
        let _ = pkt; // already correct
        let (hdr, payload) = RtpHeader::parse(&pkt).unwrap();
        assert!(hdr.extension);
        assert_eq!(payload, &[0xCC, 0xDD]);
    }
}
