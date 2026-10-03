# xmip-core-transport-zigbee

Zigbee transport: APS framing over the network layer — a Stream is one APS data frame, or a fragmented transmission of numbered blocks acknowledged by the receiver; a loopback radio stands in for the coordinator. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

A send target is read by `net::Target` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the one reading of a URI every technology calls: scheme, authority, path and decoded query. Until 2026-09-28 it was read through the transport capability's `socket::target`, which split it on its first slash and left the query in the path.

A `0x` number in a target is read by `codec::hex::prefixed_number` in [xmip-core-library-codec](https://github.com/IlleNilsson/xmip-core-library-codec), which refuses a sign; until 2026-09-28 it was read with `from_str_radix`, which took `0x+7e8`.

## Acknowledgement

The sender is acknowledged after the whole receive cycle. The APS
acknowledgement of the frame that completes a transmission, the unfragmented
frame or the last block, is sent on Accepted and withheld on Failed, so the
sender's retries send the transmission again. APS has no negative
acknowledgement (Zigbee Specification, chapter 2.2, the APS sub-layer), so nothing
tells a sender not to send again: on Refused the acknowledgement is sent, the
transmission taken and not sent again, and the refusal is what the runtime
audited. A transmission that asked for no acknowledgement waits for nobody and
is at-most-once. Each transmission arrives whole.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
