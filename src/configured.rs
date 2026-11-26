//! How a Zigbee Location is configured: `Configured`, reading the settings
//! its [`declaration`](crate::declaration) declares once, and the one way a
//! node is built from them (ADR-0064, amendments 2026-09-26 and
//! 2026-11-26).

use std::sync::Arc;

use transport::Configured;
use transport::error::{Result, protocol_error};
use xcore::settings::{Read, Settings};

use crate::declaration::{self, CLUSTER, PROFILE};
use crate::{LoopbackRadio, Radio, ZigbeeTransport};

impl Configured for ZigbeeTransport {
    const SETTINGS: &'static Settings = declaration::SETTINGS;
    const SCHEMES: &'static [&'static str] = declaration::SCHEMES;

    fn timeout_kept(&mut self) -> Option<&mut dyn transport::configured::TimeoutKept> {
        Some(&mut self.timeout)
    }

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
        let node = ZigbeeTransport::new(radio, short_address);
        Ok(node.speaking(sixteen("profile", PROFILE)?, sixteen("cluster", CLUSTER)?))
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
        assert!(ZigbeeTransport::open("wpan0", Applies::Send, &given[..1]).is_err());
    }
}
