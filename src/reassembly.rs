//! The receiving node's side of APS: it takes a data frame whole, puts the
//! blocks of a fragmented transmission back together under their counter,
//! and acknowledges what asked to be — every unfragmented frame, and the
//! last block of a transmission once every block is in. The same reassembly
//! runs inside the loopback radio and behind a real one.

use std::collections::VecDeque;

use transport::error::{Result, protocol_error};

use crate::frame::{Fragment, Frame, Header};

/// What the node has to say back, and what it has taken whole.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// Acknowledgements for the sender, in order.
    pub acks: Vec<Frame>,
    /// A transmission that just completed: its header and its bytes.
    pub complete: Option<(Header, Vec<u8>)>,
}

/// One transmission being put together.
#[derive(Debug)]
struct Transmission {
    header: Header,
    blocks: Vec<Option<Vec<u8>>>,
}

impl Transmission {
    fn complete(&self) -> bool {
        self.blocks.iter().all(Option::is_some)
    }

    fn join(self) -> (Header, Vec<u8>) {
        let bytes = self.blocks.into_iter().flatten().flatten().collect();
        (self.header, bytes)
    }
}

/// One node's APS receive side.
#[derive(Debug, Default)]
pub struct Reassembly {
    transmission: Option<Transmission>,
    complete: VecDeque<(Header, Vec<u8>)>,
}

impl Reassembly {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One frame from the radio.
    ///
    /// # Errors
    /// A block of a transmission that did not start, a block beyond the
    /// count, or a first block while another transmission is under way.
    pub fn handle(&mut self, frame: &Frame) -> Result<Response> {
        let mut response = Response::default();
        if frame.ack {
            return Ok(response);
        }
        match frame.fragment {
            None => {
                self.complete
                    .push_back((frame.header.clone(), frame.payload.clone()));
            }
            Some(Fragment::First { count }) => {
                if self.transmission.is_some() {
                    return Err(protocol_error("a first block while another is under way"));
                }
                let mut blocks = vec![None; usize::from(count).max(1)];
                blocks[0] = Some(frame.payload.clone());
                self.transmission = Some(Transmission {
                    header: frame.header.clone(),
                    blocks,
                });
            }
            Some(Fragment::Block { index }) => {
                let transmission = self.transmission.as_mut().ok_or_else(|| {
                    protocol_error("a block of a transmission that did not start")
                })?;
                if transmission.header.counter != frame.header.counter {
                    return Err(protocol_error("a block under another counter"));
                }
                let slot = transmission
                    .blocks
                    .get_mut(usize::from(index))
                    .ok_or_else(|| protocol_error("a block beyond the count"))?;
                *slot = Some(frame.payload.clone());
            }
        }
        if let Some(transmission) = self.transmission.take_if(|t| t.complete()) {
            self.complete.push_back(transmission.join());
        }
        if frame.ack_request && (frame.fragment.is_none() || self.transmission.is_none()) {
            response.acks.push(frame.acknowledging());
        }
        response.complete = self.complete.pop_front();
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    fn header() -> Header {
        Header {
            destination: 0,
            source: 0x1a2b,
            endpoint: 1,
            cluster: 0x0402,
            profile: 0x0104,
            counter: 9,
        }
    }

    #[test]
    fn blocks_come_back_together_and_the_last_is_acknowledged() {
        let mut node = Reassembly::new();
        let long: Vec<u8> = (0..300u32)
            .map(|n| u8::try_from(n % 256).unwrap_or(0))
            .collect();
        let frames = frame::frames(&header(), &long).expect("frames");
        for frame in &frames[..frames.len() - 1] {
            let response = node.handle(frame).expect("block");
            assert!(response.acks.is_empty() && response.complete.is_none());
        }
        let response = node.handle(&frames[frames.len() - 1]).expect("last");
        assert_eq!(response.acks.len(), 1);
        assert!(response.acks[0].ack);
        assert_eq!(response.complete, Some((header(), long)));
    }

    #[test]
    fn an_unfragmented_frame_is_whole_and_acknowledged_at_once() {
        let mut node = Reassembly::new();
        let frame = Frame::data(header(), None, b"on").expect("frame");
        let response = node.handle(&frame).expect("data");
        assert_eq!(response.acks.len(), 1);
        assert_eq!(response.complete, Some((header(), b"on".to_vec())));
        let response = node.handle(&frame.acknowledging()).expect("an ack");
        assert_eq!(response, Response::default(), "an ack is not data");
    }

    #[test]
    fn a_block_out_of_place_is_refused() {
        let mut node = Reassembly::new();
        let stray = Frame::data(header(), Some(Fragment::Block { index: 1 }), b"x").expect("f");
        assert!(node.handle(&stray).is_err(), "no transmission");
        let first = Frame::data(header(), Some(Fragment::First { count: 2 }), b"a").expect("f");
        node.handle(&first).expect("first");
        assert!(node.handle(&first).is_err(), "another first");
        let beyond = Frame::data(header(), Some(Fragment::Block { index: 2 }), b"x").expect("f");
        assert!(node.handle(&beyond).is_err(), "beyond the count");
        let mut other = header();
        other.counter = 10;
        let elsewhere = Frame::data(other, Some(Fragment::Block { index: 1 }), b"x").expect("f");
        assert!(node.handle(&elsewhere).is_err(), "another counter");
        let response = node.handle(&stray).expect("the block that was due");
        assert_eq!(response.complete, Some((header(), b"ax".to_vec())));
    }
}
