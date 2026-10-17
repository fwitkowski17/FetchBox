# librqbit-tracker-comms 9.0.1, patched for FetchBox

This is the unmodified crate from crates.io (Apache-2.0, by Igor Katson, <https://github.com/ikatson/rqbit>)
with **one change**, in `src/tracker_comms_udp.rs` (`UdpTrackerClient::new`, marked `FetchBox patch`):

Upstream binds the UDP tracker socket to `[::]` only. On a host whose kernel has no IPv6 at all
(`ipv6.disable=1`, some NAS and minimal kernels) that fails with "Address family not supported by
protocol", and because the session creates this client unconditionally, **no torrent could be started
on such a host**. The patch falls back to an IPv4 socket when the IPv6 one cannot be made.

It is used through `[patch.crates-io]` in the workspace `Cargo.toml`. Drop the patch (and this folder)
as soon as upstream makes the bind address configurable or falls back on its own.
