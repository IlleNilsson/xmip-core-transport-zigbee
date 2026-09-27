//! The settings a Zigbee Location takes, and the one way a node is built
//! from them (ADR-0064, amendment 2026-09-26).

use std::sync::Arc;

use transport::Configured;
use transport::error::{Result, protocol_error};
use xcore::settings::{Applies, Fixed, Kind, Presence, Read, Setting, Settings};

use crate::{CLUSTER, LoopbackRadio, PROFILE, Radio, TIMEOUT, ZigbeeTransport};

/// A sixteen-bit identifier.
const SIXTEEN: Kind = Kind::Integer {
    minimum: 0,
    maximum: 0xffff,
};

impl Configured for ZigbeeTransport {
    /// The address names the radio. `loopback`, the in-process node, is the
    /// one the estate has; an 802.15.4 radio joins when it exposes one.
    const SETTINGS: &'static Settings = &Settings {
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
            Setting {
                name: "timeout",
                kind: Kind::Duration,
                presence: Presence::Default(Fixed::Duration(TIMEOUT)),
                meaning: "How long a frame or an acknowledgement is waited for.",
                applies: Applies::Both,
            },
        ],
    };

    fn configured(address: &str, settings: &Read) -> Result<Self> {
        let radio: Arc<dyn Radio> = match address {
            "loopback" => Arc::new(LoopbackRadio::new()),
            other => {
                return Err(protocol_error(format!(
                    "{other:?} is not a radio this estate has; `loopback` is"
                )));
            }
        };
        let sixteen = |name, held: u16| {
            settings.optional_integer(name).map_or(Ok(held), |value| {
                u16::try_from(value).map_err(|_| protocol_error(format!("a {name} over 0xffff")))
            })
        };
        // A Receive Location names no profile or cluster: it takes what is
        // sent to it, so it keeps the node's own.
        let short_address = u16::try_from(settings.integer("short_address"))
            .map_err(|_| protocol_error("a short address over 0xfff7"))?;
        let node = ZigbeeTransport::new(radio, short_address)
            .timing_out_after(settings.duration("timeout"));
        Ok(node.speaking(sixteen("profile", PROFILE)?, sixteen("cluster", CLUSTER)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcore::settings::Given;

    #[test]
    fn zigbee_declares_its_settings_and_reads_through_them() {
        assert_eq!(ZigbeeTransport::SETTINGS.problems(), Vec::<String>::new());
        let given = [
            ("short_address".to_string(), Given::Integer(0x1a2b)),
            ("cluster".to_string(), Given::Integer(0x0006)),
        ];
        let built = ZigbeeTransport::open("loopback", Applies::Send, &given).expect("built");
        assert_eq!(built.address, 0x1a2b);
        assert_eq!((built.profile, built.cluster), (PROFILE, 0x0006));
        assert_eq!(built.timeout, TIMEOUT);
        let Err(refused) = ZigbeeTransport::open("loopback", Applies::Receive, &given) else {
            panic!("a Receive Location names no cluster");
        };
        assert!(refused.message.contains("cluster"), "{}", refused.message);
        assert!(ZigbeeTransport::open("wpan0", Applies::Send, &given[..1]).is_err());
    }
}
