//! Command-line argument parsing for `aprsfeed-rs`.
//!
//! Defines the [`Args`] struct which is parsed from the process arguments using `clap`.

use clap::Parser;

/// APRS igate feeder: receives AX.25 frames from a multicast RTP stream and
/// forwards APRS packets to the APRS2 igate network over TCP.
#[derive(Debug, Clone, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// APRS-IS login callsign (e.g. "N0CALL-10").
    #[arg(short = 'u', long = "user")]
    pub user: String,

    /// APRS-IS passcode.  Auto-calculated from the callsign if not supplied.
    #[arg(short = 'p', long = "passcode")]
    pub passcode: Option<u16>,

    /// Multicast group address (or hostname) carrying the RTP/AX.25 stream.
    #[arg(short = 'I', long = "input", default_value = "ax25.mcast.local")]
    pub input: String,

    /// APRS2 igate server hostname.
    #[arg(short = 'H', long = "host", default_value = "noam.aprs2.net")]
    pub host: String,

    /// APRS2 igate server TCP port.
    #[arg(short = 'P', long = "port", default_value_t = 14580)]
    pub port: u16,

    /// Source-specific multicast (SSM / IGMPv3) source address.
    ///
    /// When supplied, the socket performs an IGMPv3 INCLUDE-mode join
    /// (`IP_ADD_SOURCE_MEMBERSHIP` for IPv4, `MCAST_JOIN_SOURCE_GROUP` for
    /// IPv6) restricted to traffic originating from this address.  When
    /// omitted the socket falls back to an IGMPv2 any-source join
    /// (`IP_ADD_MEMBERSHIP`).
    #[arg(short = 's', long = "source")]
    pub source: Option<String>,

    /// Optional path to a log file.  When set, tracing events are also written
    /// to this file in addition to stderr.
    #[arg(short = 'f', long = "logfile")]
    pub logfile: Option<String>,

    /// Enable verbose (debug-level) logging.
    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    pub verbose: bool,
}
