//! How a Zigbee Location is configured: `Configured`, reading the settings
//! its [`declaration`](crate::declaration) declares once, and the one way a
//! node is built from them (ADR-0064, amendments 2026-09-26 and
//! 2026-11-26). The address names the radio, then where a send goes:
//! `wpan0/0x0002/1`, or `loopback`.

use std::sync::Arc;

use net::Target;
use transport::Configured;
use transport::error::{Result, protocol_error};
use transport::ieee802154::{self, Radio};
use xcore::settings::{Read, Settings};

use crate::declaration::{self, CLUSTER, PROFILE};
use crate::{LoopbackRadio, ZigbeeTransport};

impl Configured for ZigbeeTransport {
    const SETTINGS: &'static Settings = declaration::SETTINGS;
    const SCHEMES: &'static [&'static str] = declaration::SCHEMES;

    fn timeout_kept(&mut self) -> Option<&mut dyn transport::configured::TimeoutKept> {
        Some(&mut self.timeout)
    }

    fn configured(address: &str, settings: &Read) -> Result<Self> {
        let sixteen = |name, held: u16| {
            settings.optional_integer(name).map_or(Ok(held), |value| {
                u16::try_from(value).map_err(|_| protocol_error(format!("a {name} over 0xffff")))
            })
        };
        // A Receive Location names no profile or cluster: it takes what is
        // sent to it, so it keeps the node's own.
        let short_address = u16::try_from(settings.integer("short_address"))
            .map_err(|_| protocol_error("a short address over 0xfff7"))?;
        let named = Target::bare(address);
        let radio: Arc<dyn Radio> = match named.authority() {
            "loopback" => Arc::new(LoopbackRadio::new()),
            name => {
                let (radio, at) = ieee802154::interface(name)?;
                ieee802154::agreeing(name, "short_address", short_address, at.short)?;
                radio
            }
        };
        let mut node = ZigbeeTransport::new(radio, short_address)
            .speaking(sixteen("profile", PROFILE)?, sixteen("cluster", CLUSTER)?);
        if !named.path().is_empty() {
            (node.destination, node.endpoint) = ZigbeeTransport::node_and_endpoint(named.path())?;
        }
        Ok(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::declaration::TIMEOUT;
    use xcore::settings::{Applies, Given};

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
        // No interface of the name, no 802.15.4 sockets, or one whose own
        // short address is not 0x1a2b: each refused.
        assert!(ZigbeeTransport::open("wpan0", Applies::Send, &given[..1]).is_err());
    }

    #[test]
    fn an_address_names_the_radio_and_the_node_a_send_goes_to() {
        let given = [("short_address".to_string(), Given::Integer(1))];
        let at = "zigbee://loopback/0x0002/3";
        let built = ZigbeeTransport::open(at, Applies::Send, &given).expect("built");
        assert_eq!((built.destination, built.endpoint), (2, 3));
        let first = ZigbeeTransport::open("zigbee://loopback/0x0002", Applies::Send, &given)
            .expect("built");
        assert_eq!((first.destination, first.endpoint), (2, 1));
        let coordinator =
            ZigbeeTransport::open("zigbee://loopback", Applies::Send, &given).expect("built");
        assert_eq!((coordinator.destination, coordinator.endpoint), (0, 1));
        let bad = ZigbeeTransport::open("zigbee://loopback/two", Applies::Send, &given);
        assert!(bad.is_err(), "not a node");
    }
}
