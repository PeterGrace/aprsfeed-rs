//! Multicast UDP socket creation helpers.
//!
//! Wraps `socket2` to create a properly configured multicast UDP socket and
//! hands it to Tokio as a non-blocking async socket.
//!
//! # Multicast mode selection
//!
//! | `source` arg | IPv4 socket option          | IGMP version |
//! |--------------|-----------------------------|--------------|
//! | `None`       | `IP_ADD_MEMBERSHIP`         | IGMPv2 ASM   |
//! | `Some(addr)` | `IP_ADD_SOURCE_MEMBERSHIP`  | IGMPv3 SSM   |
//!
//! For IPv6 the equivalent MLDv1 (`IPV6_JOIN_GROUP`) / MLDv2
//! (`MCAST_JOIN_SOURCE_GROUP`) options are used instead.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::mem;
use std::os::unix::io::AsRawFd;

use anyhow::{Context, Result, bail};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

/// Mirror of the POSIX `group_source_req` struct from `<netinet/in.h>`.
///
/// This type is not exposed by the `libc` crate, so we define it manually.
/// The layout must exactly match the kernel ABI (`#[repr(C)]` ensures this).
/// Used by `MCAST_JOIN_SOURCE_GROUP` for MLDv2 (IPv6 SSM) joins.
#[repr(C)]
struct GroupSourceReq {
    /// Network interface index (0 = default interface).
    gsr_interface: u32,
    /// Multicast group address as a generic socket address storage.
    gsr_group: libc::sockaddr_storage,
    /// Permitted source address as a generic socket address storage.
    gsr_source: libc::sockaddr_storage,
}

/// Default RTP port used by ka9q-radio for AX.25 multicast streams.
pub const DEFAULT_RTP_PORT: u16 = 5004;

/// Create and return a bound, multicast-joined async UDP socket.
///
/// Resolves `mcast_addr_str` via Tokio's async DNS resolver, creates a
/// `socket2` socket with `SO_REUSEADDR` and `SO_REUSEPORT` set (allowing
/// multiple receivers on the same multicast group/port), joins the multicast
/// group, then converts to a Tokio `UdpSocket`.
///
/// When `source` is `Some`, an IGMPv3 source-specific multicast (SSM) join is
/// performed, restricting reception to traffic from that source address.  When
/// `source` is `None`, an IGMPv2 any-source join is used.
///
/// # Arguments
///
/// * `mcast_addr_str` - Hostname or IP address of the multicast group.
/// * `port`           - UDP port to bind and receive on.
/// * `source`         - Optional SSM source address.  Must match the address
///   family of the resolved multicast group.
///
/// # Returns
///
/// A ready-to-use [`tokio::net::UdpSocket`] joined to the multicast group.
///
/// # Errors
///
/// Returns an error if DNS resolution fails, the multicast group address is
/// invalid, the source address family does not match the group, socket
/// creation fails, or the join fails.
pub async fn create_multicast_socket(
    mcast_addr_str: &str,
    port: u16,
    source: Option<IpAddr>,
) -> Result<UdpSocket> {
    // Resolve the multicast hostname to a SocketAddr.
    let target = format!("{mcast_addr_str}:{port}");
    let mut addrs = tokio::net::lookup_host(&target)
        .await
        .with_context(|| format!("DNS lookup failed for {target}"))?;

    let addr = addrs
        .next()
        .with_context(|| format!("No addresses resolved for {target}"))?;

    match addr {
        SocketAddr::V4(v4) => {
            // Validate that the source, if given, is also IPv4.
            let src = match source {
                Some(IpAddr::V4(s)) => Some(s),
                Some(IpAddr::V6(s)) => bail!(
                    "Address family mismatch: IPv6 source {s} specified for IPv4 group {mcast_addr_str}"
                ),
                None => None,
            };
            create_v4_socket(*v4.ip(), port, src)
        }
        SocketAddr::V6(v6) => {
            // Validate that the source, if given, is also IPv6.
            let src = match source {
                Some(IpAddr::V6(s)) => Some(s),
                Some(IpAddr::V4(s)) => bail!(
                    "Address family mismatch: IPv4 source {s} specified for IPv6 group {mcast_addr_str}"
                ),
                None => None,
            };
            create_v6_socket(*v6.ip(), v6.scope_id(), port, src)
        }
    }
}

/// Build an IPv4 multicast socket joined to `mcast_addr` on `port`.
///
/// Uses `IP_ADD_SOURCE_MEMBERSHIP` (IGMPv3 SSM) when `source` is `Some`,
/// falling back to `IP_ADD_MEMBERSHIP` (IGMPv2 ASM) when `source` is `None`.
fn create_v4_socket(mcast_addr: Ipv4Addr, port: u16, source: Option<Ipv4Addr>) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
        .context("Failed to create IPv4 UDP socket")?;

    // Allow multiple processes to bind the same multicast address+port.
    socket
        .set_reuse_address(true)
        .context("set_reuse_address failed")?;
    socket
        .set_reuse_port(true)
        .context("set_reuse_port failed")?;

    // Bind to the multicast group address (not INADDR_ANY) so the kernel only
    // delivers datagrams destined for this specific group.  On Linux with
    // SO_REUSEPORT, binding to INADDR_ANY causes the socket to receive traffic
    // for *every* multicast group that any local process has joined on the same
    // port — not just the group this socket joined via IP_ADD_MEMBERSHIP.
    let bind_addr: SocketAddr = (IpAddr::V4(mcast_addr), port).into();
    socket
        .bind(&bind_addr.into())
        .with_context(|| format!("Failed to bind to {bind_addr}"))?;

    // Choose SSM or ASM join based on whether a source address was supplied.
    if let Some(src) = source {
        join_ssm_v4(&socket, mcast_addr, src)?;
    } else {
        socket
            .join_multicast_v4(&mcast_addr, &Ipv4Addr::UNSPECIFIED)
            .with_context(|| format!("Failed to join multicast group {mcast_addr}"))?;
    }

    socket
        .set_nonblocking(true)
        .context("set_nonblocking failed")?;

    // Convert the std socket into a Tokio socket.
    // SAFETY: we just created this socket and set it non-blocking; no other
    // owner exists, so `from_std` is safe here.
    UdpSocket::from_std(socket.into()).context("Failed to convert socket to Tokio UdpSocket")
}

/// Build an IPv6 multicast socket joined to `mcast_addr` on `port`.
///
/// Uses `MCAST_JOIN_SOURCE_GROUP` (MLDv2 SSM) when `source` is `Some`,
/// falling back to `IPV6_JOIN_GROUP` (MLDv1 ASM) when `source` is `None`.
fn create_v6_socket(
    mcast_addr: Ipv6Addr,
    scope_id: u32,
    port: u16,
    source: Option<Ipv6Addr>,
) -> Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
        .context("Failed to create IPv6 UDP socket")?;

    socket
        .set_reuse_address(true)
        .context("set_reuse_address failed")?;
    socket
        .set_reuse_port(true)
        .context("set_reuse_port failed")?;

    // Bind to the multicast group address (not ::) for the same reason as the
    // IPv4 path: SO_REUSEPORT on Linux leaks other groups' traffic when bound
    // to the unspecified address.
    let bind_addr: SocketAddr = (IpAddr::V6(mcast_addr), port).into();
    socket
        .bind(&bind_addr.into())
        .with_context(|| format!("Failed to bind to {bind_addr}"))?;

    if let Some(src) = source {
        join_ssm_v6(&socket, mcast_addr, scope_id, src)?;
    } else {
        socket
            .join_multicast_v6(&mcast_addr, scope_id)
            .with_context(|| format!("Failed to join IPv6 multicast group {mcast_addr}"))?;
    }

    socket
        .set_nonblocking(true)
        .context("set_nonblocking failed")?;

    UdpSocket::from_std(socket.into()).context("Failed to convert socket to Tokio UdpSocket")
}

/// Perform an IGMPv3 source-specific multicast join for an IPv4 group.
///
/// Issues `IP_ADD_SOURCE_MEMBERSHIP` with an `ip_mreq_source` struct, which
/// instructs the kernel to emit an IGMPv3 INCLUDE-mode report for
/// `(mcast_addr, source_addr)` on the default interface.
///
/// # Arguments
///
/// * `socket`      - The UDP socket to configure.
/// * `mcast_addr`  - Multicast group address (224.0.0.0/4).
/// * `source_addr` - Permitted source address for the SSM join.
///
/// # Errors
///
/// Returns an error if the `setsockopt` call fails.
fn join_ssm_v4(socket: &Socket, mcast_addr: Ipv4Addr, source_addr: Ipv4Addr) -> Result<()> {
    // `u32::from_ne_bytes(octets)` stores the address exactly as the kernel
    // expects in `in_addr.s_addr` — i.e. the raw network-byte-order bytes
    // reinterpreted as a native u32 without swapping.  This matches what
    // socket2 does internally for `join_multicast_v4`.
    let mreq_source = libc::ip_mreq_source {
        imr_multiaddr: libc::in_addr {
            s_addr: u32::from_ne_bytes(mcast_addr.octets()),
        },
        imr_interface: libc::in_addr {
            // INADDR_ANY: let the kernel pick the interface.
            s_addr: u32::from_ne_bytes(Ipv4Addr::UNSPECIFIED.octets()),
        },
        imr_sourceaddr: libc::in_addr {
            s_addr: u32::from_ne_bytes(source_addr.octets()),
        },
    };

    // SAFETY: `mreq_source` is a valid, fully-initialised `ip_mreq_source`
    // whose lifetime spans the setsockopt call.  The cast to `*const c_void`
    // and the explicit size argument are the required setsockopt calling
    // convention on all POSIX platforms.
    let ret = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_ADD_SOURCE_MEMBERSHIP,
            &mreq_source as *const _ as *const libc::c_void,
            mem::size_of::<libc::ip_mreq_source>() as libc::socklen_t,
        )
    };

    if ret != 0 {
        bail!(
            "IP_ADD_SOURCE_MEMBERSHIP failed for group {mcast_addr}, source {source_addr}: {}",
            std::io::Error::last_os_error()
        );
    }

    Ok(())
}

/// Perform an MLDv2 source-specific multicast join for an IPv6 group.
///
/// Issues `MCAST_JOIN_SOURCE_GROUP` with a `group_source_req` struct, which
/// instructs the kernel to emit an MLDv2 INCLUDE-mode report for
/// `(mcast_addr, source_addr)`.
///
/// # Arguments
///
/// * `socket`      - The UDP socket to configure.
/// * `mcast_addr`  - Multicast group address (ff00::/8).
/// * `scope_id`    - Interface index / scope ID for the multicast group.
/// * `source_addr` - Permitted source address for the SSM join.
///
/// # Errors
///
/// Returns an error if the `setsockopt` call fails.
fn join_ssm_v6(
    socket: &Socket,
    mcast_addr: Ipv6Addr,
    scope_id: u32,
    source_addr: Ipv6Addr,
) -> Result<()> {
    // `group_source_req` embeds two `sockaddr_storage` fields for the group
    // and source.  We fill them by copying a `sockaddr_in6` into each field —
    // sockaddr_storage is guaranteed to be large enough to hold any sockaddr.
    let group_sa = libc::sockaddr_in6 {
        sin6_family: libc::AF_INET6 as libc::sa_family_t,
        sin6_port: 0,
        sin6_flowinfo: 0,
        sin6_addr: libc::in6_addr { s6_addr: mcast_addr.octets() },
        sin6_scope_id: scope_id,
    };
    let source_sa = libc::sockaddr_in6 {
        sin6_family: libc::AF_INET6 as libc::sa_family_t,
        sin6_port: 0,
        sin6_flowinfo: 0,
        sin6_addr: libc::in6_addr { s6_addr: source_addr.octets() },
        sin6_scope_id: 0,
    };

    // SAFETY: `group_source_req` is a C-layout struct; zeroing it produces a
    // valid zero-value for all fields (interface index 0 = default interface).
    let mut gsreq: GroupSourceReq = unsafe { mem::zeroed() };
    gsreq.gsr_interface = scope_id;

    // SAFETY: `sockaddr_storage` is at least as large as `sockaddr_in6`
    // (mandated by POSIX), so the byte-copy cannot overflow the destination.
    // The source objects are stack-allocated and live through the copy.
    unsafe {
        std::ptr::copy_nonoverlapping(
            &group_sa as *const _ as *const u8,
            &mut gsreq.gsr_group as *mut _ as *mut u8,
            mem::size_of::<libc::sockaddr_in6>(),
        );
        std::ptr::copy_nonoverlapping(
            &source_sa as *const _ as *const u8,
            &mut gsreq.gsr_source as *mut _ as *mut u8,
            mem::size_of::<libc::sockaddr_in6>(),
        );
    }

    // SAFETY: `gsreq` is fully initialised above; pointer and size arguments
    // follow the setsockopt convention.
    let ret = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IPV6,
            libc::MCAST_JOIN_SOURCE_GROUP,
            &gsreq as *const _ as *const libc::c_void,
            mem::size_of::<GroupSourceReq>() as libc::socklen_t,
        )
    };

    if ret != 0 {
        bail!(
            "MCAST_JOIN_SOURCE_GROUP failed for group {mcast_addr}, source {source_addr}: {}",
            std::io::Error::last_os_error()
        );
    }

    Ok(())
}
