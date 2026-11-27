# xmip-core-transport-zigbee

Zigbee transport: APS framing over the network layer — a Stream is one APS data frame, or a fragmented transmission of numbered blocks acknowledged by the receiver; an 802.15.4 interface of Linux's kernel, or a loopback radio in process. A technology of [xmip-core-transport](https://github.com/IlleNilsson/xmip-core-transport).

A send target is read by `net::Target` in [xmip-core-library-net](https://github.com/IlleNilsson/xmip-core-library-net), the one reading of a URI every technology calls: scheme, authority, path and decoded query. Until 2026-09-28 it was read through the transport capability's `socket::target`, which split it on its first slash and left the query in the path.

A `0x` number in a target is read by `codec::hex::prefixed_number` in [xmip-core-library-codec](https://github.com/IlleNilsson/xmip-core-library-codec), which refuses a sign; until 2026-09-28 it was read with `from_str_radix`, which took `0x+7e8`.

## The radio

The radio is the transport capability's `ieee802154::Radio`, and a frame's
MAC header and check sequence are the radio's: this crate hands it the
network frame, to the short address the network header names — one hop, no
route — and takes frames back with the short address that sent them
(ADR-0051, amendment 2026-11-27). A Location's address names the radio:

- **an interface of Linux's kernel**, `zigbee://wpan0`, an `AF_IEEE802154`
  datagram socket bound to the interface's own PAN and short address
  (`net::ieee802154::Interface`), which a user opens with no capability.
  The Location's `short_address` must be the interface's, since the
  kernel's MAC sends from it; another is refused, naming both. Elsewhere,
  and on a Linux whose kernel carries no 802.15.4 sockets, it is refused in
  words, so `xmip-service` links Zigbee on Linux alone;
- **`loopback`**, the receiving node in process, the coordinator
  reassembling and acknowledging, which every test and the System Tests
  drive (`src/loopback.rs`).

A Send Location's address may name the node and endpoint it sends to after
the radio, as a target does: `zigbee://wpan0/0x0002/1`; the coordinator's
first endpoint where it names none. Proven through a node on Linux's two
virtual radios, `mac802154_hwsim`'s `wpan0` and `wpan1`, by the estate's
service test (`aired.rs`), the far end acknowledging as it takes. What one
frame carries above the MAC, `frame::MAX_NPDU`, is the radio's
`net::ieee802154::MAX_PAYLOAD`; until 2026-11-27 it was written here as a
number, and the crate had no radio but the loopback.

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

## The declaration

What a Location of this technology is configured with is two `const`s in its `declaration` module, `src/declaration.rs`, beside the code, which `Configured` reads (ADR-0064, amendment 2026-11-26): `declaration::SETTINGS`, `short_address`, `profile`, `cluster` and `timeout`, and `SCHEMES`, `zigbee`. The profile, the cluster and the wait `profile`, `cluster` and `timeout` default to, `PROFILE`, `CLUSTER` and `TIMEOUT`, are there too, and the code reads them from there, where they were the crate root's until then. It is the same wherever it is built, a box with no 802.15.4 radio included. Built with its `declaration` feature alone (`--no-default-features --features declaration`), the crate is those and nothing more: its code and every crate the code is built on are behind the default feature. So the library the surfaces load carries it without the code, through the gathering crate xmip-core-declaration (open problem 48), and the language server and the desktop suggest and check a Zigbee Location's settings though no node in their process loaded the technology. `test/Declaration.Test.ps1` in the estate holds that the declaration builds nothing the transport capability does not.

## Toolchain

`rust-toolchain.toml` pins the toolchain for the whole estate. Do not change it
here.

## Verification

The included workflow is manual-only and calls the versioned shared workflow at
`IlleNilsson/.github@v1`.
