//! Multicast UDP socket creation helpers.
//!
//! Wraps `socket2` to create a properly configured multicast UDP socket and
//! hands it to Tokio as a non-blocking async socket.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{Context, Result};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

/// Default RTP port used by ka9q-radio for AX.25 multicast streams.
pub const DEFAULT_RTP_PORT: u16 = 5004;

/// Create and return a bound, multicast-joined async UDP socket.
///
/// Resolves `mcast_addr_str` via Tokio's async DNS resolver, creates a
/// `socket2` socket with `SO_REUSEADDR` and `SO_REUSEPORT` set (allowing
/// multiple receivers on the same multicast group/port), joins the multicast
/// group on the wildcard interface, then converts to a Tokio `UdpSocket`.
///
/// # Arguments
///
/// * `mcast_addr_str` - Hostname or IP address of the multicast group.
/// * `port`           - UDP port to bind and receive on.
///
/// # Returns
///
/// A ready-to-use [`tokio::net::UdpSocket`] joined to the multicast group.
///
/// # Errors
///
/// Returns an error if DNS resolution fails, the multicast group address is
/// invalid, socket creation fails, or the join fails.
pub async fn create_multicast_socket(mcast_addr_str: &str, port: u16) -> Result<UdpSocket> {
    // Resolve the multicast hostname to a SocketAddr.
    let target = format!("{mcast_addr_str}:{port}");
    let mut addrs = tokio::net::lookup_host(&target)
        .await
        .with_context(|| format!("DNS lookup failed for {target}"))?;

    let addr = addrs
        .next()
        .with_context(|| format!("No addresses resolved for {target}"))?;

    match addr {
        SocketAddr::V4(v4) => create_v4_socket(*v4.ip(), port),
        SocketAddr::V6(v6) => create_v6_socket(*v6.ip(), v6.scope_id(), port),
    }
}

/// Build an IPv4 multicast socket joined to `mcast_addr` on port `port`.
fn create_v4_socket(mcast_addr: Ipv4Addr, port: u16) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("Failed to create IPv4 UDP socket")?;

    // Allow multiple processes to bind the same multicast address+port.
    socket
        .set_reuse_address(true)
        .context("set_reuse_address failed")?;
    socket
        .set_reuse_port(true)
        .context("set_reuse_port failed")?;

    // Bind to INADDR_ANY:port so we receive multicast traffic on all interfaces.
    let bind_addr: SocketAddr = (IpAddr::V4(Ipv4Addr::UNSPECIFIED), port).into();
    socket
        .bind(&bind_addr.into())
        .with_context(|| format!("Failed to bind to {bind_addr}"))?;

    // Join the multicast group on the default interface.
    socket
        .join_multicast_v4(&mcast_addr, &Ipv4Addr::UNSPECIFIED)
        .with_context(|| format!("Failed to join multicast group {mcast_addr}"))?;

    socket
        .set_nonblocking(true)
        .context("set_nonblocking failed")?;

    // Convert the std socket into a Tokio socket.
    // SAFETY: we just created this socket and set it non-blocking; no other
    // owner exists, so `from_std` is safe here.
    UdpSocket::from_std(socket.into()).context("Failed to convert socket to Tokio UdpSocket")
}

/// Build an IPv6 multicast socket joined to `mcast_addr` on port `port`.
fn create_v6_socket(mcast_addr: Ipv6Addr, scope_id: u32, port: u16) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
        .context("Failed to create IPv6 UDP socket")?;

    socket
        .set_reuse_address(true)
        .context("set_reuse_address failed")?;
    socket
        .set_reuse_port(true)
        .context("set_reuse_port failed")?;

    let bind_addr: SocketAddr = (IpAddr::V6(Ipv6Addr::UNSPECIFIED), port).into();
    socket
        .bind(&bind_addr.into())
        .with_context(|| format!("Failed to bind to {bind_addr}"))?;

    socket
        .join_multicast_v6(&mcast_addr, scope_id)
        .with_context(|| format!("Failed to join IPv6 multicast group {mcast_addr}"))?;

    socket
        .set_nonblocking(true)
        .context("set_nonblocking failed")?;

    UdpSocket::from_std(socket.into()).context("Failed to convert socket to Tokio UdpSocket")
}
