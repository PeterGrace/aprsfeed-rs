//! APRS-IS iGate TCP connection management and packet forwarding.
//!
//! The iGate task maintains a persistent (auto-reconnecting) TCP connection to
//! an APRS-IS server, logs in with the operator's credentials, and forwards
//! formatted APRS packets received from the processing pipeline.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter},
    net::TcpStream,
    sync::mpsc,
    time::sleep,
};
use tracing::{debug, error, info, warn};

/// Run the iGate forwarding loop, reconnecting indefinitely on failure.
///
/// Attempts to connect to the APRS-IS server at `host:port`, authenticate with
/// `user`/`passcode`, and forward every `String` received from `rx`.  On
/// disconnect or error the task waits 10 minutes before attempting to reconnect
/// so as not to hammer the server.
///
/// # Arguments
///
/// * `host`     - APRS-IS server hostname (e.g. `"noam.aprs2.net"`).
/// * `port`     - Server TCP port (typically `14580`).
/// * `user`     - Operator callsign used for login.
/// * `passcode` - 15-bit APRS-IS passcode for `user`.
/// * `rx`       - Channel receiver from which formatted APRS packets are read.
pub async fn run_igate(
    host: String,
    port: u16,
    user: String,
    passcode: u16,
    mut rx: mpsc::Receiver<String>,
) {
    loop {
        match connect_and_run(&host, port, &user, passcode, &mut rx).await {
            Ok(()) => info!("iGate connection closed cleanly; scheduling reconnect"),
            Err(e) => error!("iGate error: {e}; reconnecting in 10 minutes"),
        }
        // Back-off to avoid hammering the server after a failure.
        sleep(Duration::from_secs(600)).await;
    }
}

/// Attempt to connect to `host:port` with up to 10 retries at 500 ms intervals.
///
/// # Returns
///
/// A connected [`TcpStream`], or an error if all attempts are exhausted.
async fn resolve_with_retry(host: &str, port: u16) -> Result<TcpStream> {
    const MAX_TRIES: u32 = 10;
    const BACKOFF: Duration = Duration::from_millis(500);

    let target = format!("{host}:{port}");

    for attempt in 1..=MAX_TRIES {
        // Re-resolve on every attempt so we pick up DNS changes / round-robin
        // entries across retries.
        match tokio::net::lookup_host(&target).await {
            Err(e) => {
                warn!("DNS lookup failed (attempt {attempt}/{MAX_TRIES}): {e}");
                sleep(BACKOFF).await;
                continue;
            }
            Ok(addrs) => {
                let addrs: Vec<_> = addrs.collect();
                if addrs.is_empty() {
                    warn!("No addresses for {target} (attempt {attempt}/{MAX_TRIES})");
                    sleep(BACKOFF).await;
                    continue;
                }

                // Try each resolved address in order.
                let mut last_err: Option<std::io::Error> = None;
                for addr in &addrs {
                    match TcpStream::connect(addr).await {
                        Ok(stream) => {
                            info!("Connected to iGate server {addr}");
                            return Ok(stream);
                        }
                        Err(e) => {
                            debug!("Connect to {addr} failed: {e}");
                            last_err = Some(e);
                        }
                    }
                }

                // Convert the error to an owned String before the borrow
                // checker requires us to drop `last_err`.
                let err_str = last_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                warn!(
                    "All addresses for {target} failed \
                     (attempt {attempt}/{MAX_TRIES}): {err_str}"
                );
            }
        }

        sleep(BACKOFF).await;
    }

    Err(anyhow!(
        "Failed to connect to {target} after {MAX_TRIES} attempts"
    ))
}

/// Establish one TCP session with the APRS-IS server and run until disconnect.
///
/// Sends the login banner, then multiplexes between:
/// * Lines arriving from the server (logged at debug level).
/// * Packets arriving from the channel (`rx`), written to the server.
///
/// # Errors
///
/// Returns `Err` on any I/O failure or server-initiated disconnect.
async fn connect_and_run(
    host: &str,
    port: u16,
    user: &str,
    passcode: u16,
    rx: &mut mpsc::Receiver<String>,
) -> Result<()> {
    let stream = resolve_with_retry(host, port)
        .await
        .with_context(|| format!("Could not reach {host}:{port}"))?;

    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half).lines();
    let mut writer = BufWriter::new(write_half);

    // Send APRS-IS login line.
    let login = format!(
        "user {user} pass {passcode} vers aprsfeed-rs {}\r\n",
        env!("CARGO_PKG_VERSION"),
    );
    writer
        .write_all(login.as_bytes())
        .await
        .context("Failed to send login")?;
    writer.flush().await.context("Failed to flush login")?;
    info!("Sent login for {user} to {host}:{port}");

    loop {
        tokio::select! {
            // ------------------------------------------------------------------
            // Branch A: data from the server (banner lines, keepalive comments).
            // ------------------------------------------------------------------
            line_result = reader.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        debug!("Server: {line}");
                    }
                    Ok(None) => {
                        return Err(anyhow!("Server closed connection"));
                    }
                    Err(e) => {
                        return Err(e).context("Error reading from server");
                    }
                }
            }

            // ------------------------------------------------------------------
            // Branch B: a formatted APRS packet from the processing pipeline.
            // ------------------------------------------------------------------
            packet_opt = rx.recv() => {
                match packet_opt {
                    Some(packet) => {
                        let line = format!("{packet}\r\n");
                        writer
                            .write_all(line.as_bytes())
                            .await
                            .context("Failed to write packet")?;
                        writer.flush().await.context("Failed to flush packet")?;
                        debug!("Sent: {packet}");
                    }
                    None => {
                        // Sender side of the channel was dropped → clean shutdown.
                        info!("Packet channel closed; disconnecting from iGate");
                        return Ok(());
                    }
                }
            }
        }
    }
}
