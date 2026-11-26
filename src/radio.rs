//! Where a node's frames go and come from: the [`Radio`], the air as one
//! node hears it, and [`LoopbackRadio`], the receiving node in-process,
//! which every test and every box without an 802.15.4 radio drives.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use transport::error::Result;
use xcore::lock::Locked;

use crate::frame::{Frame, Header};
use crate::reassembly::Reassembly;

/// Where frames go and come from: the air, as one node hears it.
pub trait Radio: Send + Sync {
    /// The radio's name, for the origin URI.
    fn name(&self) -> &str;
    /// Put a frame on the air.
    ///
    /// # Errors
    /// Where the radio refused it.
    fn transmit(&self, frame: &[u8]) -> Result<()>;
    /// The next frame, or `None` when nothing arrived within `timeout`.
    ///
    /// # Errors
    /// Where the radio could not be read.
    fn receive(&self, timeout: Duration) -> Result<Option<Vec<u8>>>;
}

/// The receiving node in-process: what the sender transmits, the node
/// reassembles and acknowledges, and the acknowledgement is what the
/// sender receives next.
#[derive(Default)]
pub struct LoopbackRadio {
    node: Mutex<Reassembly>,
    to_sender: Mutex<VecDeque<Vec<u8>>>,
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

    fn transmit(&self, bytes: &[u8]) -> Result<()> {
        let frame = Frame::decode(bytes)?;
        let response = self.node.locked().handle(&frame)?;
        self.to_sender
            .locked()
            .extend(response.acks.iter().map(Frame::encode));
        if let Some(complete) = response.complete {
            self.arrived.locked().push_back(complete);
        }
        Ok(())
    }

    fn receive(&self, _timeout: Duration) -> Result<Option<Vec<u8>>> {
        Ok(self.to_sender.locked().pop_front())
    }
}
