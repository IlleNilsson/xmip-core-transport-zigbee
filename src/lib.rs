#![forbid(unsafe_code)]

//! Streams that arrive over Zigbee. One APS data frame is one Stream where
//! it fits; a longer Stream is a fragmented transmission — numbered blocks
//! under one APS counter, acknowledged once every block is in — and arrives
//! whole.
//!
//! Zigbee is the mesh of the building's sensors and switches: 802.15.4
//! radios in a network of short addresses, a coordinator at its root, and
//! above the network layer the application support sublayer that names an
//! endpoint, a profile and a cluster for every frame. What is here is the
//! network and APS headers, the extended header that fragments a
//! transmission, and the acknowledgement. A Send Location addresses a node
//! and endpoint; a Receive Location is a node taking what is sent to it.
//!
//! **The sender is acknowledged after the whole receive cycle.** The APS
//! acknowledgement of the frame that completes a transmission is sent on
//! [`transport::Verdict::Accepted`] and withheld on
//! [`transport::Verdict::Failed`], so the sender's retries send it again.
//! APS has no negative acknowledgement (Zigbee Specification, chapter 2.2,
//! the APS sub-layer), so nothing tells a
//! sender *refused, do not send again*: on [`transport::Verdict::Refused`]
//! the acknowledgement is sent, the transmission taken and not sent again,
//! and the refusal is what the runtime audited. A transmission that asked for no
//! acknowledgement is at-most-once ([`AT_MOST_ONCE`]). Each arrives whole.
//!
//! The radio is a trait: [`LoopbackRadio`] is the receiving node in-process,
//! which every test and every box without an 802.15.4 radio drives, the way
//! can-bus drives its loopback bus. The origin URI names the radio, the
//! sender and the cluster: `zigbee://loopback/0x1a2b/1?cluster=0x0402`.

pub mod frame;
pub mod reassembly;
mod settings;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use codec::hex::prefixed_number;
pub use frame::{Fragment, Frame, Header, MAX_STREAM};
use net::Target;
pub use reassembly::Reassembly;
use transport::error::{Result, TransportError, protocol_error};
use transport::held::Held;
use transport::loopback::{FarEnd, LOOPBACK_TIMEOUT, Loopback};
use transport::{Acknowledgement, Arrived, Directions, Taken, Transport, Verdict};

/// Why a transmission that asked for no acknowledgement cannot be
/// acknowledged after the receive cycle.
pub const AT_MOST_ONCE: &str = "a Zigbee APS frame that asks for no acknowledgement waits for \
                                nobody: the sender sent it once";

/// The profile a node speaks unless told otherwise: Home Automation.
pub const PROFILE: u16 = 0x0104;
/// The cluster a node speaks unless told otherwise: temperature measurement.
pub const CLUSTER: u16 = 0x0402;
/// How long a node waits for a frame or an acknowledgement unless told
/// otherwise.
pub const TIMEOUT: Duration = Duration::from_secs(5);

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
        self.arrived
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
    }
}

impl Radio for LoopbackRadio {
    fn name(&self) -> &'static str {
        "loopback"
    }

    fn transmit(&self, bytes: &[u8]) -> Result<()> {
        let frame = Frame::decode(bytes)?;
        let response = self
            .node
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .handle(&frame)?;
        self.to_sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(response.acks.iter().map(Frame::encode));
        if let Some(complete) = response.complete {
            self.arrived
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_back(complete);
        }
        Ok(())
    }

    fn receive(&self, _timeout: Duration) -> Result<Option<Vec<u8>>> {
        Ok(self
            .to_sender
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front())
    }
}

/// One node on one radio: a short address, an endpoint, and the profile and
/// cluster it speaks.
#[derive(Clone)]
pub struct ZigbeeTransport {
    radio: Arc<dyn Radio>,
    address: u16,
    endpoint: u8,
    profile: u16,
    cluster: u16,
    destination: u16,
    /// The APS counter, shared by every clone: a transmission is known by it.
    counter: Arc<Mutex<u8>>,
    timeout: Duration,
    /// Set on a loopback: the radio holds what the node took.
    loopback: Option<Arc<LoopbackRadio>>,
}

impl ZigbeeTransport {
    /// A node at `address` on `radio`, endpoint 1, [`PROFILE`] and
    /// [`CLUSTER`], sending to the coordinator.
    #[must_use]
    pub fn new(radio: Arc<dyn Radio>, address: u16) -> Self {
        Self {
            radio,
            address,
            endpoint: 1,
            profile: PROFILE,
            cluster: CLUSTER,
            destination: 0x0000,
            counter: Arc::new(Mutex::new(0)),
            timeout: TIMEOUT,
            loopback: None,
        }
    }

    /// Speak `cluster` in `profile`.
    #[must_use]
    pub fn speaking(mut self, profile: u16, cluster: u16) -> Self {
        self.profile = profile;
        self.cluster = cluster;
        self
    }

    /// Give up on an acknowledgement that does not come within `timeout`.
    #[must_use]
    pub const fn timing_out_after(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// `zigbee://<radio>/<address>/<endpoint>`.
    #[must_use]
    pub fn origin(&self, address: u16, endpoint: u8) -> String {
        format!("zigbee://{}/{address:#06x}/{endpoint}", self.radio.name())
    }

    fn next_counter(&self) -> u8 {
        let mut counter = self.counter.lock().unwrap_or_else(PoisonError::into_inner);
        let next = *counter;
        *counter = counter.wrapping_add(1);
        next
    }

    /// Send `bytes` to `destination` as one transmission and wait for its
    /// acknowledgement.
    ///
    /// # Errors
    /// More than a transmission carries, a radio that refused a frame, or
    /// an acknowledgement that did not come or names another transmission.
    pub fn send_stream(&self, destination: u16, endpoint: u8, bytes: &[u8]) -> Result<()> {
        let header = Header {
            destination,
            source: self.address,
            endpoint,
            cluster: self.cluster,
            profile: self.profile,
            counter: self.next_counter(),
        };
        for frame in frame::frames(&header, bytes)? {
            self.radio.transmit(&frame.encode())?;
        }
        let ack = self
            .radio
            .receive(self.timeout)?
            .ok_or_else(|| TransportError::retryable("no acknowledgement came"))?;
        let ack = Frame::decode(&ack)?;
        if !ack.ack || ack.header.counter != header.counter {
            return Err(protocol_error("an acknowledgement of something else"));
        }
        Ok(())
    }

    /// Take one transmission sent to this node, whole, or `None` when
    /// nothing arrived in time. The APS acknowledgement of the frame that
    /// completes it is the arrival's verdict: sent on accepted, withheld on
    /// refused, so the sender's APS retries send it again. A transmission
    /// that asked for no acknowledgement is at-most-once ([`AT_MOST_ONCE`]).
    ///
    /// # Errors
    /// Where the radio could not be read or a frame was out of place.
    pub fn receive_one(&self) -> Result<Option<Arrived>> {
        let mut node = Reassembly::new();
        let Some(first) = self.radio.receive(self.timeout)? else {
            return Ok(None);
        };
        let mut bytes = first;
        loop {
            let response = node.handle(&Frame::decode(&bytes)?)?;
            if let Some((header, payload)) = response.complete {
                let acknowledgement = if response.acks.is_empty() {
                    Acknowledgement::at_most_once(AT_MOST_ONCE)
                } else {
                    let radio = Arc::clone(&self.radio);
                    let acks = response.acks;
                    Acknowledgement::deferred(move |verdict| {
                        // APS has no negative acknowledgement: a refused
                        // transmission is acknowledged, taken for good.
                        if matches!(verdict, Verdict::Accepted | Verdict::Refused(_)) {
                            for ack in &acks {
                                radio.transmit(&ack.encode())?;
                            }
                        }
                        Ok(())
                    })
                };
                return Ok(Some(Arrived::whole(
                    self.arrival(&header),
                    payload,
                    acknowledgement,
                )));
            }
            for ack in &response.acks {
                self.radio.transmit(&ack.encode())?;
            }
            bytes = self
                .radio
                .receive(self.timeout)?
                .ok_or_else(|| TransportError::retryable("the sender went quiet"))?;
        }
    }

    fn arrival(&self, header: &Header) -> String {
        format!(
            "{}?cluster={:#06x}",
            self.origin(header.source, header.endpoint),
            header.cluster
        )
    }
}

impl Transport for ZigbeeTransport {
    fn name(&self) -> &'static str {
        "zigbee"
    }

    fn directions(&self) -> Directions {
        Directions::BOTH
    }

    fn arrivals(&self) -> transport::Arrivals {
        transport::Arrivals::Ordered("one line or bus, answered in the order it speaks")
    }

    /// Nothing on the air is not an error: an empty vector. The APS
    /// acknowledgement waits for the receive cycle: sent on accepted,
    /// withheld on refused.
    fn receive(&self) -> Result<Vec<Arrived>> {
        Ok(self.receive_one()?.into_iter().collect())
    }

    /// `target` may name a node and endpoint, `zigbee://radio/0x0000/1`,
    /// overriding the transport's.
    fn send(&self, target: &str, bytes: &[u8]) -> Result<()> {
        let (destination, endpoint) = match Target::under(&["zigbee"], target)
            .map(|named| (named.authority(), named.path()))
        {
            Some((_, path)) if !path.is_empty() => {
                let (address, endpoint) = path.split_once('/').unwrap_or((path, "1"));
                let refused = || protocol_error(format!("{path:?} is not a node and endpoint"));
                (
                    prefixed_number(address).map_err(|_| refused())?,
                    endpoint.parse().map_err(|_| refused())?,
                )
            }
            _ => (self.destination, self.endpoint),
        };
        self.send_stream(destination, endpoint, bytes)
    }
}

impl ZigbeeTransport {
    /// Both ends on one in-process radio: a node at `0x1a2b` sending to the
    /// coordinator, and the coordinator taking and acknowledging, the
    /// loopback timeout on the acknowledgement.
    #[must_use]
    pub fn loopback() -> Self {
        let radio = Arc::new(LoopbackRadio::new());
        let mut transport = Self::new(Arc::clone(&radio) as Arc<dyn Radio>, 0x1a2b)
            .timing_out_after(LOOPBACK_TIMEOUT);
        transport.loopback = Some(radio);
        transport
    }
}

impl Loopback for ZigbeeTransport {
    /// One byte counts the blocks and one block carries 98 bytes.
    fn ceiling(&self) -> Option<usize> {
        Some(MAX_STREAM)
    }

    /// The coordinator, holding the transmission it took whole.
    fn far_end(&self) -> Result<Box<dyn FarEnd>> {
        let radio = self
            .loopback
            .as_ref()
            .ok_or_else(|| protocol_error("a radio, not a loopback radio"))?;
        let transport = self.clone();
        let radio = Arc::clone(radio);
        Ok(Box::new(Held::new(
            self.origin(self.destination, self.endpoint),
            move || {
                let (header, bytes) = radio
                    .take()
                    .ok_or_else(|| protocol_error("no transmission completed"))?;
                Ok(Taken::new(transport.arrival(&header), bytes))
            },
        )))
    }

    fn send_to(&self, address: &str, payload: &[u8]) -> Result<()> {
        self.clone().send(address, payload)
    }

    /// In order on one thread: the coordinator lives in the radio and
    /// acknowledges as the node sends, so the send goes first and the take
    /// finds the transmission whole.
    fn exchanges_in_order(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use transport::payload::{edge_payloads, patterned};

    /// The shapes a protocol breaks on, as the Playground lists them, up to
    /// the ceiling and one over it.
    fn payloads() -> Vec<(&'static str, Vec<u8>)> {
        let mut payloads = edge_payloads();
        payloads.extend([
            ("mtu", patterned(1_472)),
            ("the brim", patterned(MAX_STREAM)),
        ]);
        payloads
    }

    #[test]
    fn a_loopback_round_carries_a_stream_as_one_transmission() {
        let loopback = ZigbeeTransport::loopback();
        let arrived = loopback.round(b"21.5").expect("round");
        assert_eq!(arrived.bytes, b"21.5");
        assert_eq!(
            arrived.origin_uri,
            "zigbee://loopback/0x1a2b/1?cluster=0x0402"
        );
        let long = vec![7; 1000];
        assert_eq!(loopback.round(&long).expect("blocks").bytes, long);
        assert_eq!(loopback.ceiling(), Some(24_990));
        assert!(loopback.refuses(b"anything").is_none());
        assert_eq!(loopback.name(), "zigbee");
        assert!(loopback.directions().receives() && loopback.directions().sends());
        assert!(loopback.claims().is_none());
    }

    #[test]
    fn the_loopback_returns_the_edges_whole_and_refuses_over_the_brim() {
        let loopback = ZigbeeTransport::loopback();
        for (name, bytes) in payloads() {
            let arrived = loopback
                .round(&bytes)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(arrived.bytes, bytes, "{name}");
        }
        let error = loopback.round(&vec![0; MAX_STREAM + 1]).expect_err("over");
        assert!(error.message.contains("over the 24990"), "{error}");
    }

    #[test]
    fn a_target_names_the_node_and_endpoint_and_a_bad_one_is_refused() {
        let loopback = ZigbeeTransport::loopback();
        loopback
            .send("zigbee://loopback/0x0007/3", b"on")
            .expect("sending");
        let radio = loopback.loopback.as_ref().expect("loopback");
        let (header, bytes) = radio.take().expect("taken");
        assert_eq!((header.destination, header.endpoint), (7, 3));
        assert_eq!(bytes, b"on");
        assert!(loopback.send("zigbee://loopback/seven", b"x").is_err());
        assert!(loopback.send("zigbee://loopback/0x0007/x", b"x").is_err());
    }

    #[test]
    fn the_node_takes_a_transmission_over_any_radio() {
        // Two ends of one air: what one transmits, the other receives. The
        // sender runs on another thread and the node takes the transmission.
        struct Air {
            up: Mutex<VecDeque<Vec<u8>>>,
            down: Mutex<VecDeque<Vec<u8>>>,
        }
        struct End(Arc<Air>, bool);
        impl Radio for End {
            fn name(&self) -> &'static str {
                "air"
            }
            fn transmit(&self, frame: &[u8]) -> Result<()> {
                let queue = if self.1 { &self.0.down } else { &self.0.up };
                queue.lock().expect("lock").push_back(frame.to_vec());
                Ok(())
            }
            fn receive(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
                let queue = if self.1 { &self.0.up } else { &self.0.down };
                let deadline = std::time::Instant::now() + timeout;
                loop {
                    if let Some(frame) = queue.lock().expect("lock").pop_front() {
                        return Ok(Some(frame));
                    }
                    if std::time::Instant::now() > deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        let air = Arc::new(Air {
            up: Mutex::new(VecDeque::new()),
            down: Mutex::new(VecDeque::new()),
        });
        let node = ZigbeeTransport::new(Arc::new(End(Arc::clone(&air), true)), 0)
            .timing_out_after(Duration::from_secs(2));
        assert!(node.far_end().is_err(), "a real radio has no node inside");
        assert!(
            node.receive().expect("quiet").is_empty(),
            "nothing is not an error"
        );
        let sender = ZigbeeTransport::new(Arc::new(End(air, false)), 0x1a2b)
            .speaking(0x0104, 0x0006)
            .timing_out_after(Duration::from_millis(200));
        let sending = std::thread::spawn(move || {
            // Refused: acknowledged, taken for good, so the send succeeds.
            sender.send("zigbee://air", &[7; 300])?;
            let refused = sender
                .send("zigbee://air", &[9; 300])
                .expect_err("no acknowledgement");
            sender.send("zigbee://air", &[9; 300])?;
            Ok::<_, TransportError>(refused)
        });
        let refused = node.receive().expect("taking").remove(0);
        assert!(refused.defers(), "the sender waits for its acknowledgement");
        refused
            .refused(transport::Refusal::Unacceptable)
            .expect("acknowledged");
        // Failed: the acknowledgement is withheld, and the sender retries.
        let first = node.receive().expect("taking").remove(0);
        first.failed().expect("failed");
        let mut arrived = node.receive().expect("taking again");
        assert_eq!(arrived.len(), 1);
        let arrived = arrived.remove(0).taken().expect("acknowledged");
        let refused = sending.join().expect("thread").expect("sent again");
        assert!(refused.retryable, "{refused}");
        assert_eq!(arrived.bytes, [9; 300]);
        assert_eq!(arrived.origin_uri, "zigbee://air/0x1a2b/1?cluster=0x0006");
    }
}
