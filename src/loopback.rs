//! [`LoopbackRadio`], the receiving node in process: a radio
//! (`transport::ieee802154::Radio`) every test and every box without an
//! 802.15.4 radio drives.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use net::ieee802154::Heard;
use transport::Waiting;
use transport::error::Result;
use transport::ieee802154::Radio;
use xcore::lock::Locked;

use crate::frame::{Frame, Header};
use crate::reassembly::Reassembly;

/// The receiving node in-process: what the sender transmits, the node
/// reassembles and acknowledges, and the acknowledgement is what the
/// sender receives next, from the short address it sent to.
#[derive(Default)]
pub struct LoopbackRadio {
    node: Mutex<Reassembly>,
    to_sender: Mutex<VecDeque<Heard>>,
    arrived: Mutex<VecDeque<(Header, Vec<u8>)>>,
}

impl LoopbackRadio {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The next transmission the node took whole.
    #[must_use]
    pub fn take(&self) -> Option<(Header, Vec<u8>)> {
        self.arrived.locked().pop_front()
    }
}

impl Radio for LoopbackRadio {
    fn name(&self) -> &'static str {
        "loopback"
    }

    /// The node takes every frame, whatever short address it names, and
    /// answers from that address: it is the one node on its air.
    fn transmit(&self, destination: u16, bytes: &[u8]) -> Result<()> {
        let frame = Frame::decode(bytes)?;
        let response = self.node.locked().handle(&frame)?;
        self.to_sender
            .locked()
            .extend(response.acks.iter().map(|ack| Heard {
                source: destination,
                payload: ack.encode(),
            }));
        if let Some(complete) = response.complete {
            self.arrived.locked().push_back(complete);
        }
        Ok(())
    }

    fn receive(&self, _timeout: Duration) -> Result<Option<Heard>> {
        Ok(self.to_sender.locked().pop_front())
    }

    fn waiting(&self) -> Waiting {
        Waiting::Polled("a read of the loopback radio returns at once")
    }
}
