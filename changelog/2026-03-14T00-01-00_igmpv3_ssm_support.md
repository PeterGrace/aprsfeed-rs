# IGMPv3 / MLDv2 Source-Specific Multicast Support

**Date:** 2026-03-14T00:01:00

## Summary

Added source-specific multicast (SSM) support so that the socket can perform
an IGMPv3 INCLUDE-mode join when a source address is provided via the new
`-s` / `--source` CLI flag.  When the flag is omitted the existing IGMPv2
any-source behaviour is preserved.

## Motivation

Some network deployments configure multicast switches/routers in SSM mode,
where only IGMPv3 INCLUDE reports are honoured.  The prior implementation
always issued an IGMPv2 `IP_ADD_MEMBERSHIP` join, which would be silently
ignored or cause unexpected traffic on such networks.

## Changes

### `Cargo.toml`

- Added `libc = "0.2"` dependency (used for raw `setsockopt` calls that are
  not yet exposed by `socket2`).

### `src/cli.rs`

- Added `--source` / `-s` `Option<String>` argument.  When supplied, the
  value is parsed as an `IpAddr` and forwarded to the multicast layer.

### `src/main.rs`

- Parse `args.source` into `Option<IpAddr>` with a descriptive error on
  parse failure.
- Pass `source` as the new third argument to `create_multicast_socket`.
- Log join mode ("IGMPv3 SSM" vs "IGMPv2 ASM") at `info` level, including
  the source address when SSM is active.

### `src/multicast.rs`

- `create_multicast_socket` now accepts `source: Option<IpAddr>` and
  validates that the source address family matches the resolved group family,
  returning an error on mismatch.
- `create_v4_socket` dispatches to `join_ssm_v4` (IGMPv3) or
  `join_multicast_v4` (IGMPv2) based on whether `source` is `Some`.
- `create_v6_socket` dispatches to `join_ssm_v6` (MLDv2) or
  `join_multicast_v6` (MLDv1) based on whether `source` is `Some`.
- **New** `join_ssm_v4`: calls `setsockopt(IP_ADD_SOURCE_MEMBERSHIP)` with an
  `ip_mreq_source` struct (available in `libc`).
- **New** `join_ssm_v6`: calls `setsockopt(MCAST_JOIN_SOURCE_GROUP)` with a
  `group_source_req` struct.  Because `libc` does not expose this struct, a
  local `#[repr(C)] GroupSourceReq` mirrors the kernel ABI exactly.
- All `unsafe` blocks carry SAFETY comments explaining the invariants.

## Behaviour Matrix

| `--source` flag | IPv4 socket option          | Protocol version |
|-----------------|-----------------------------|------------------|
| omitted         | `IP_ADD_MEMBERSHIP`         | IGMPv2 ASM       |
| provided        | `IP_ADD_SOURCE_MEMBERSHIP`  | IGMPv3 SSM       |
| omitted         | `IPV6_JOIN_GROUP`           | MLDv1 ASM        |
| provided        | `MCAST_JOIN_SOURCE_GROUP`   | MLDv2 SSM        |

## Usage

```sh
# IGMPv2 any-source join (unchanged behaviour)
aprsfeed-rs -u N0CALL-10 -I ax25.mcast.local

# IGMPv3 source-specific join
aprsfeed-rs -u N0CALL-10 -I ax25.mcast.local -s 192.168.1.50
```
