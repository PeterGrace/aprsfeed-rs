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

    /// Optional path to a log file.  When set, tracing events are also written
    /// to this file in addition to stderr.
    #[arg(short = 'f', long = "logfile")]
    pub logfile: Option<String>,

    /// Enable verbose (debug-level) logging.
    #[arg(short = 'v', long = "verbose", default_value_t = false)]
    pub verbose: bool,
}
