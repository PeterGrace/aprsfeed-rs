# Fix: Bind to Multicast Group Address Instead of INADDR_ANY

**Date:** 2026-03-14T19:40:00

## Problem

When running on a machine hosting multiple ka9q-radio streams (e.g. `hackrf-w3pog-pcm.local`, `packet.local`, `ft4-pcm.local`, `ft8-pcm.local`), the app received a flood of RTP packets with unexpected payload types (PT=122, PCM audio) even though it had only joined the `ax25.local` multicast group (`239.221.127.106`).

**Root cause:** On Linux with `SO_REUSEPORT`, a UDP socket bound to `INADDR_ANY:5004` receives multicast datagrams for *every* group joined by *any* process on the machine at that port — not just the group this socket joined via `IP_ADD_MEMBERSHIP`. All ka9q-radio streams share port 5004, so all their traffic leaked into this socket.

## Fix

Changed the bind address in both `create_v4_socket` and `create_v6_socket` from the unspecified address (`0.0.0.0` / `::`) to the resolved multicast group address itself (e.g. `239.221.127.106:5004`). The kernel then filters at the bind level and delivers only datagrams destined for that specific group.

## Files Changed

- `src/multicast.rs`: `create_v4_socket` and `create_v6_socket` — bind to `mcast_addr:port` instead of `INADDR_ANY:port`.
