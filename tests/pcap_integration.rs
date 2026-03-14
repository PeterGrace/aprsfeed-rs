//! Integration test: feed a real captured AX.25 multicast packet through the
//! full decoding pipeline and verify the resulting APRS-IS TNC2 string.
//!
//! The fixture `aprspacket.pcap` must exist at the repository root.  It was
//! captured with `tcpdump` on a Linux host, so the link type is
//! LINKTYPE_LINUX_SLL2 (276).  The test asserts both structural TNC2 validity
//! and the exact field values decoded from that known packet.

use std::fs;

use pcap_file::pcap::PcapReader;
use pcap_file::DataLink;

use aprsfeed_rs::pipeline::process_packet;

/// Path to the pcap fixture, relative to the package root where `cargo test`
/// sets the working directory.
const PCAP_PATH: &str = "aprspacket.pcap";

/// Expected link type for this fixture (captured with Linux cooked capture v2).
const EXPECTED_DATALINK: DataLink = DataLink::LINUX_SLL2;

/// Expected UDP destination port for the ka9q-radio AX.25 RTP stream.
const RTP_PORT: u16 = 5004;

/// Station callsign injected into the TNC2 `qAO` gate path during testing.
const TEST_STATION: &str = "N0CALL";

#[test]
fn pcap_roundtrip() {
    let file = fs::File::open(PCAP_PATH)
        .unwrap_or_else(|e| panic!("cannot open '{PCAP_PATH}': {e}"));

    let mut reader =
        PcapReader::new(file).unwrap_or_else(|e| panic!("invalid pcap file: {e}"));

    // Confirm the link type so failures are immediately diagnosable.
    assert_eq!(
        reader.header().datalink,
        EXPECTED_DATALINK,
        "unexpected link type in fixture (expected Linux SLL2)"
    );

    let mut decoded: Vec<String> = Vec::new();

    while let Some(record) = reader.next_packet() {
        let pkt = record.unwrap_or_else(|e| panic!("malformed pcap record: {e}"));

        // Extract the UDP payload from the captured frame.
        let Some(udp_payload) = extract_udp_payload_sll2(&pkt.data) else {
            continue;
        };

        // Feed the raw RTP datagram through the complete pipeline.
        if let Some(aprs_line) = process_packet(udp_payload, TEST_STATION) {
            decoded.push(aprs_line);
        }
    }

    assert!(
        !decoded.is_empty(),
        "no APRS packets decoded – check that '{PCAP_PATH}' contains \
         AX.25 UI frames on UDP port {RTP_PORT}"
    );

    // -----------------------------------------------------------------------
    // Structural validation – every decoded line must be valid TNC2 format.
    // -----------------------------------------------------------------------
    for line in &decoded {
        let arrow = line
            .find('>')
            .unwrap_or_else(|| panic!("missing '>' separator in TNC2 line: {line}"));
        let colon = line
            .find(':')
            .unwrap_or_else(|| panic!("missing ':' separator in TNC2 line: {line}"));

        // Correct order: SOURCE > DEST … : INFO
        assert!(arrow < colon, "malformed field order in TNC2 line: {line}");
        assert!(
            line.contains(",qAO,"),
            "gate path token qAO missing from: {line}"
        );

        let info = &line[colon + 1..];
        assert!(!info.is_empty(), "empty APRS info field in: {line}");
    }

    // -----------------------------------------------------------------------
    // Exact-value assertions for the known single-packet fixture.
    //
    // The captured datagram decodes to:
    //   Source  : W3POG-7
    //   Dest    : T0QVLZ      (MIC-E encoded destination)
    //   Digis   : WIDE1-1, WIDE2-1 (neither repeated)
    //   Control : 0x03 (UI)
    //   PID     : 0xF0 (no layer-3)
    //   Info    : MIC-E position report (starts with "'")
    // -----------------------------------------------------------------------
    assert_eq!(
        decoded.len(),
        1,
        "fixture contains exactly one APRS packet; got {}",
        decoded.len()
    );

    let line = &decoded[0];

    assert!(
        line.starts_with("W3POG-7>T0QVLZ,"),
        "unexpected source or destination callsign: {line}"
    );
    assert!(
        line.contains("WIDE1-1,WIDE2-1,"),
        "expected digipeaters WIDE1-1 and WIDE2-1 in path: {line}"
    );
    assert!(
        line.contains(",qAO,N0CALL:"),
        "gate path not injected correctly: {line}"
    );

    // The info field must not be empty and must not contain a raw CR/LF/NUL.
    let info = line.split(':').nth(1).expect("colon already validated above");
    assert!(!info.is_empty(), "info field is empty");
    assert!(
        !info.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0),
        "info field contains unstripped control characters: {info:?}"
    );
}

/// Extract the UDP payload from a Linux SLL2-encapsulated frame.
///
/// Walks the fixed-size headers (SLL2 → IPv4 → UDP) and returns a slice of
/// the UDP payload, or `None` if:
/// * The frame is too short or the protocol at any layer is unexpected.
/// * The UDP destination port is not [`RTP_PORT`].
///
/// # Arguments
///
/// * `data` - Raw captured frame bytes starting at the SLL2 header.
fn extract_udp_payload_sll2(data: &[u8]) -> Option<&[u8]> {
    // Linux SLL2 header layout (20 bytes):
    //   [0:2]   EtherType / protocol (big-endian)
    //   [2:4]   reserved
    //   [4:8]   interface index
    //   [8:10]  ARPHRD type
    //   [10]    packet type
    //   [11]    source address length
    //   [12:20] source address (padded to 8 bytes)
    const SLL2_LEN: usize = 20;
    const ETHERTYPE_IPV4: u16 = 0x0800;
    const IP_PROTO_UDP: u8 = 17;

    if data.len() < SLL2_LEN {
        return None;
    }

    // Only handle IPv4 for now.
    let ethertype = u16::from_be_bytes([data[0], data[1]]);
    if ethertype != ETHERTYPE_IPV4 {
        return None;
    }

    let ip = &data[SLL2_LEN..];

    // IPv4: minimum 20-byte fixed header.
    // IHL (internet header length) is the low nibble of the first byte,
    // in units of 32-bit words; multiply by 4 to get bytes.
    if ip.len() < 20 {
        return None;
    }
    let ihl = ((ip[0] & 0x0f) as usize) * 4;
    if ip[9] != IP_PROTO_UDP || ip.len() < ihl + 8 {
        return None;
    }

    let udp = &ip[ihl..];

    // UDP header (8 bytes): [0:2] src_port, [2:4] dst_port,
    //                        [4:6] length (header + payload), [6:8] checksum.
    let dst_port = u16::from_be_bytes([udp[2], udp[3]]);
    if dst_port != RTP_PORT {
        return None;
    }

    // udp_len includes the 8-byte header itself; payload starts at byte 8.
    let udp_len = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if udp_len < 8 || udp.len() < udp_len {
        return None;
    }

    Some(&udp[8..udp_len])
}
