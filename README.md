# xmip-core-transport-zigbee

Zigbee transport: APS framing over the network layer — a Stream is one APS data frame, or a fragmented transmission of numbered blocks acknowledged by the receiver; a loopback radio stands in for the coordinator. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
