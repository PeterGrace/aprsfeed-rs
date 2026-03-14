//! `aprsfeed-rs` – APRS igate feeder.
//!
//! Receives AX.25 packet frames from a multicast RTP stream produced by
//! `ka9q-radio`, decodes them, formats them as APRS-IS TNC2 monitor strings,
//! and forwards them to an APRS2 igate server over TCP.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────┐     mpsc channel      ┌─────────────────────────┐
//! │   UDP recv loop (main)  │  ─────────────────►   │   iGate task (tokio)    │
//! │  parse RTP + AX.25      │     String packets     │  TCP → APRS-IS server   │
//! └─────────────────────────┘                        └─────────────────────────┘
//! ```

// Tokio's task-local and instrumentation macros live in `tracing`.
use std::fs::OpenOptions;
use std::net::IpAddr;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// `cli` is binary-specific (argument parsing) so it lives only in this crate.
mod cli;

// All other modules are provided by the library crate.
use aprsfeed_rs::{aprs, igate, multicast, pipeline};

use cli::Args;

/// Maximum number of formatted APRS packets buffered in the channel between
/// the UDP receive loop and the iGate TCP writer.  Excess packets are dropped
/// with a warning rather than blocking the receiver.
const CHANNEL_CAPACITY: usize = 1000;

/// Entry point.  Wires up tracing, resolves configuration, creates the
/// multicast socket, spawns the iGate task, and runs the receive loop.
#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if present (errors are non-fatal – the file is optional).
    let _ = dotenvy::dotenv();

    let args = Args::parse();

    // -----------------------------------------------------------------------
    // Configure tracing subscriber.
    // -----------------------------------------------------------------------
    let level = if args.verbose { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    let registry = tracing_subscriber::registry().with(filter);

    if let Some(ref log_path) = args.logfile {
        // Append log lines to a file AND to stderr.
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .with_context(|| format!("Failed to open log file: {log_path}"))?;

        let file_layer = fmt::layer().with_writer(file).with_ansi(false);
        let stderr_layer = fmt::layer().with_writer(std::io::stderr);

        registry.with(stderr_layer).with(file_layer).init();
    } else {
        registry.with(fmt::layer()).init();
    }

    info!("aprsfeed-rs starting");

    // -----------------------------------------------------------------------
    // Resolve APRS-IS passcode.
    // -----------------------------------------------------------------------
    let passcode = args
        .passcode
        .unwrap_or_else(|| aprs::calculate_passcode(&args.user));
    info!(user = %args.user, passcode, "APRS-IS credentials");

    // -----------------------------------------------------------------------
    // Bind multicast UDP socket.
    // -----------------------------------------------------------------------

    // Parse the optional SSM source address string into a typed IpAddr.
    let source: Option<IpAddr> = args
        .source
        .as_deref()
        .map(|s| s.parse().with_context(|| format!("Invalid source address: {s}")))
        .transpose()?;

    let socket = multicast::create_multicast_socket(
        &args.input,
        multicast::DEFAULT_RTP_PORT,
        source,
    )
    .await
    .with_context(|| {
        format!(
            "Failed to join multicast group {} on port {}",
            args.input,
            multicast::DEFAULT_RTP_PORT
        )
    })?;

    match source {
        Some(src) => info!(
            group = %args.input,
            port = multicast::DEFAULT_RTP_PORT,
            %src,
            "Joined multicast group (IGMPv3 SSM)"
        ),
        None => info!(
            group = %args.input,
            port = multicast::DEFAULT_RTP_PORT,
            "Joined multicast group (IGMPv2 ASM)"
        ),
    }

    // -----------------------------------------------------------------------
    // Create the bounded channel connecting the receive loop to the iGate task.
    // -----------------------------------------------------------------------
    let (tx, rx) = mpsc::channel::<String>(CHANNEL_CAPACITY);

    // -----------------------------------------------------------------------
    // Spawn the iGate task.  It runs independently and reconnects on failure.
    // -----------------------------------------------------------------------
    let igate_host = args.host.clone();
    let igate_port = args.port;
    let igate_user = args.user.clone();
    let igate_passcode = passcode;

    tokio::spawn(async move {
        igate::run_igate(igate_host, igate_port, igate_user, igate_passcode, rx).await;
    });

    // -----------------------------------------------------------------------
    // Main receive loop.
    // -----------------------------------------------------------------------
    // Allocate a single buffer; UDP datagrams are at most 65 535 bytes.
    let mut buf = vec![0u8; 65_535];

    loop {
        let n = socket.recv(&mut buf).await.context("UDP recv failed")?;

        let datagram = &buf[..n];

        if let Some(packet) = pipeline::process_packet(datagram, &args.user) {
            match tx.try_send(packet) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(dropped)) => {
                    warn!("iGate channel full; dropping packet: {dropped}");
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    error!("iGate channel closed unexpectedly; exiting");
                    break;
                }
            }
        }
    }

    Ok(())
}

