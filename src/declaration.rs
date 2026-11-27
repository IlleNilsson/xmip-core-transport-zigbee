//! This technology's declaration: the transport guide's "The declaration",
//! `module/core/capability/transport/doc/adding-a-transport.md`. It is the
//! same wherever it is built, a box with no 802.15.4 radio included: a
//! Location's settings are suggested and checked wherever it is designed.

use std::time::Duration;

use xcore::settings::{Applies, Fixed, Kind, Presence, Setting, Settings};

/// The profile a node speaks unless told otherwise: Home Automation.
pub const PROFILE: u16 = 0x0104;
/// The cluster a node speaks unless told otherwise: temperature measurement.
pub const CLUSTER: u16 = 0x0402;
/// How long a node waits for a frame or an acknowledgement unless told
/// otherwise.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// A sixteen-bit identifier.
const SIXTEEN: Kind = Kind::Integer {
    minimum: 0,
    maximum: 0xffff,
};

/// Every setting a Location takes beyond its address, which names the
/// radio: `loopback`, the in-process node, is the one the estate has; an
/// 802.15.4 radio joins when it exposes one.
pub const SETTINGS: &Settings = &Settings {
    technology: env!("CARGO_PKG_NAME"),
    settings: &[
        Setting {
            name: "short_address",
            kind: Kind::Integer {
                minimum: 0,
                maximum: 0xfff7,
            },
            presence: Presence::Required,
            meaning: "This node's short address in the network.",
            applies: Applies::Both,
        },
        Setting {
            name: "profile",
            kind: SIXTEEN,
            presence: Presence::Default(Fixed::Integer(PROFILE as i64)),
            meaning: "The application profile a sent frame names.",
            applies: Applies::Send,
        },
        Setting {
            name: "cluster",
            kind: SIXTEEN,
            presence: Presence::Default(Fixed::Integer(CLUSTER as i64)),
            meaning: "The cluster a sent frame names.",
            applies: Applies::Send,
        },
        transport::configured::timeout(
            Applies::Both,
            Some(TIMEOUT),
            "How long a frame or an acknowledgement is waited for.",
        ),
    ],
};

/// This technology's schemes, as [`transport::Configured::SCHEMES`] defines
/// them.
pub const SCHEMES: &[&str] = &["zigbee"];
