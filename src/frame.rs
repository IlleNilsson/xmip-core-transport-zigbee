//! The Zigbee frame as the radio carries it: the network header — frame
//! control, destination and source short addresses, radius, sequence — and
//! the APS header — frame control, destination endpoint, cluster, profile,
//! source endpoint, APS counter — with the extended header a fragmented
//! transmission adds: a fragmentation code and a block number. The first
//! block's number field carries the count of blocks; every other block's
//! carries its index.

use transport::ceiling;
use transport::error::{Result, protocol_error};

/// What the network layer may hand the MAC: a 127-byte PHY packet less the
/// eleven bytes of a MAC header with short addresses and its check sequence.
pub const MAX_NPDU: usize = 116;
/// The network header: two of frame control, two addresses, radius, sequence.
pub const NWK_HEADER: usize = 8;
/// The APS header of a data frame without the extended header.
pub const APS_HEADER: usize = 8;
/// What one unfragmented data frame carries.
pub const MAX_UNFRAGMENTED: usize = MAX_NPDU - NWK_HEADER - APS_HEADER;
/// What one block of a fragmented transmission carries: two bytes less, for
/// the extended header.
pub const MAX_BLOCK: usize = MAX_UNFRAGMENTED - 2;
/// The most blocks one transmission has: the count travels in one byte.
pub const MAX_BLOCKS: usize = 255;
/// The largest Stream a fragmented transmission carries whole.
pub const MAX_STREAM: usize = MAX_BLOCKS * MAX_BLOCK;

/// Network frame control: data frame, protocol version 2.
const NWK_DATA: [u8; 2] = [0x08, 0x00];
/// APS frame control bits.
const APS_TYPE_DATA: u8 = 0x00;
const APS_TYPE_ACK: u8 = 0x02;
const APS_ACK_REQUEST: u8 = 0x40;
const APS_EXTENDED: u8 = 0x80;
/// Extended frame control: the fragmentation code in the low two bits.
const FRAGMENT_FIRST: u8 = 0x01;
const FRAGMENT_OTHER: u8 = 0x02;

/// Where a frame goes, from where, and on what: the two headers' names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub destination: u16,
    pub source: u16,
    pub endpoint: u8,
    pub cluster: u16,
    pub profile: u16,
    pub counter: u8,
}

/// Which block of a fragmented transmission a frame is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fragment {
    /// The first block, saying how many there are.
    First { count: u8 },
    /// Any later block, saying which it is.
    Block { index: u8 },
}

/// One frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub header: Header,
    /// An APS acknowledgement rather than data.
    pub ack: bool,
    /// Data that asks to be acknowledged.
    pub ack_request: bool,
    pub fragment: Option<Fragment>,
    pub payload: Vec<u8>,
}

impl Frame {
    /// A data frame, refusing what the MAC will not carry.
    ///
    /// # Errors
    /// More than [`MAX_UNFRAGMENTED`] whole, or [`MAX_BLOCK`] in a block.
    pub fn data(header: Header, fragment: Option<Fragment>, payload: &[u8]) -> Result<Self> {
        let limit = if fragment.is_some() {
            MAX_BLOCK
        } else {
            MAX_UNFRAGMENTED
        };
        if payload.len() > limit {
            return Err(protocol_error("more than one frame carries"));
        }
        Ok(Self {
            header,
            ack: false,
            ack_request: true,
            fragment,
            payload: payload.to_vec(),
        })
    }

    /// The acknowledgement of a frame: back the way it came, under the same
    /// counter, naming the same block.
    #[must_use]
    pub fn acknowledging(&self) -> Self {
        Self {
            header: Header {
                destination: self.header.source,
                source: self.header.destination,
                ..self.header.clone()
            },
            ack: true,
            ack_request: false,
            fragment: self.fragment,
            payload: Vec::new(),
        }
    }

    /// The frame as the radio carries it.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = NWK_DATA.to_vec();
        out.extend_from_slice(&self.header.destination.to_le_bytes());
        out.extend_from_slice(&self.header.source.to_le_bytes());
        out.push(30);
        out.push(self.header.counter);
        let mut control = if self.ack {
            APS_TYPE_ACK
        } else {
            APS_TYPE_DATA
        };
        if self.ack_request {
            control |= APS_ACK_REQUEST;
        }
        if self.fragment.is_some() {
            control |= APS_EXTENDED;
        }
        out.push(control);
        out.push(self.header.endpoint);
        out.extend_from_slice(&self.header.cluster.to_le_bytes());
        out.extend_from_slice(&self.header.profile.to_le_bytes());
        out.push(self.header.endpoint);
        out.push(self.header.counter);
        match self.fragment {
            Some(Fragment::First { count }) => out.extend_from_slice(&[FRAGMENT_FIRST, count]),
            Some(Fragment::Block { index }) => out.extend_from_slice(&[FRAGMENT_OTHER, index]),
            None => {}
        }
        out.extend_from_slice(&self.payload);
        out
    }

    /// Exactly one frame.
    ///
    /// # Errors
    /// More than the MAC carries, a frame cut off inside its headers, a
    /// network frame that is not data, or a fragmentation code Zigbee does
    /// not use.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_NPDU {
            return Err(protocol_error("more than the MAC carries"));
        }
        let cut = || protocol_error("a frame cut off inside its headers");
        let (nwk, rest) = bytes.split_at_checked(NWK_HEADER).ok_or_else(cut)?;
        if nwk[..2] != NWK_DATA {
            return Err(protocol_error("a network frame that is not data"));
        }
        let (aps, rest) = rest.split_at_checked(APS_HEADER).ok_or_else(cut)?;
        let control = aps[0];
        let (fragment, payload) = if control & APS_EXTENDED != 0 {
            let (extended, payload) = rest.split_at_checked(2).ok_or_else(cut)?;
            let fragment = match extended[0] & 0x03 {
                FRAGMENT_FIRST => Fragment::First { count: extended[1] },
                FRAGMENT_OTHER => Fragment::Block { index: extended[1] },
                other => {
                    return Err(protocol_error(format!(
                        "a fragmentation code Zigbee does not use: {other}"
                    )));
                }
            };
            (Some(fragment), payload)
        } else {
            (None, rest)
        };
        Ok(Self {
            header: Header {
                destination: u16::from_le_bytes([nwk[2], nwk[3]]),
                source: u16::from_le_bytes([nwk[4], nwk[5]]),
                endpoint: aps[1],
                cluster: u16::from_le_bytes([aps[2], aps[3]]),
                profile: u16::from_le_bytes([aps[4], aps[5]]),
                counter: aps[7],
            },
            ack: control & 0x03 == APS_TYPE_ACK,
            ack_request: control & APS_ACK_REQUEST != 0,
            fragment,
            payload: payload.to_vec(),
        })
    }
}

/// `bytes` as the frames of one transmission under `header`: one data frame
/// where it fits, else the blocks of a fragmented transmission.
///
/// # Errors
/// More than [`MAX_STREAM`], which is more blocks than one byte counts.
pub fn frames(header: &Header, bytes: &[u8]) -> Result<Vec<Frame>> {
    if bytes.len() <= MAX_UNFRAGMENTED {
        return Ok(vec![Frame::data(header.clone(), None, bytes)?]);
    }
    ceiling::within(bytes.len(), MAX_STREAM, "a fragmented transmission carries")?;
    let count = u8::try_from(bytes.len().div_ceil(MAX_BLOCK)).unwrap_or(u8::MAX);
    bytes
        .chunks(MAX_BLOCK)
        .enumerate()
        .map(|(index, block)| {
            let fragment = if index == 0 {
                Fragment::First { count }
            } else {
                Fragment::Block {
                    index: u8::try_from(index).unwrap_or(u8::MAX),
                }
            };
            Frame::data(header.clone(), Some(fragment), block)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            destination: 0x0000,
            source: 0x1a2b,
            endpoint: 1,
            cluster: 0x0402,
            profile: 0x0104,
            counter: 9,
        }
    }

    #[test]
    fn a_data_frame_is_the_two_headers_and_the_payload() {
        let frame = Frame::data(header(), None, b"21.5").expect("frame");
        let bytes = frame.encode();
        assert_eq!(&bytes[..8], &[0x08, 0x00, 0x00, 0x00, 0x2b, 0x1a, 30, 9]);
        assert_eq!(&bytes[8..16], &[0x40, 1, 0x02, 0x04, 0x04, 0x01, 1, 9]);
        assert_eq!(&bytes[16..], b"21.5");
        assert_eq!(Frame::decode(&bytes).expect("decode"), frame);
        let ack = frame.acknowledging();
        assert!(ack.ack && !ack.ack_request && ack.payload.is_empty());
        assert_eq!((ack.header.destination, ack.header.source), (0x1a2b, 0));
        assert_eq!(Frame::decode(&ack.encode()).expect("decode"), ack);
    }

    #[test]
    fn a_fragmented_transmission_counts_its_blocks_in_the_first() {
        let long: Vec<u8> = (0..250u32)
            .map(|n| u8::try_from(n % 256).unwrap_or(0))
            .collect();
        let frames = frames(&header(), &long).expect("frames");
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].fragment, Some(Fragment::First { count: 3 }));
        assert_eq!(frames[1].fragment, Some(Fragment::Block { index: 1 }));
        assert_eq!(frames[2].fragment, Some(Fragment::Block { index: 2 }));
        assert_eq!(frames[2].payload.len(), 250 - 2 * MAX_BLOCK);
        let bytes = frames[0].encode();
        assert_eq!(bytes.len(), MAX_NPDU);
        assert_eq!(&bytes[16..18], &[FRAGMENT_FIRST, 3]);
        assert_eq!(Frame::decode(&bytes).expect("decode"), frames[0]);
        let whole = super::frames(&header(), &[0; MAX_UNFRAGMENTED]).expect("whole");
        assert_eq!(whole.len(), 1);
        assert!(whole[0].fragment.is_none());
        assert_eq!(super::frames(&header(), &[]).expect("empty")[0].payload, []);
    }

    #[test]
    fn what_the_mac_will_not_carry_is_refused() {
        assert!(Frame::data(header(), None, &[0; MAX_UNFRAGMENTED + 1]).is_err());
        let first = Some(Fragment::First { count: 1 });
        assert!(Frame::data(header(), first, &[0; MAX_BLOCK + 1]).is_err());
        assert!(frames(&header(), &vec![0; MAX_STREAM]).is_ok());
        assert!(frames(&header(), &vec![0; MAX_STREAM + 1]).is_err());
        let bytes = Frame::data(header(), first, b"x").expect("frame").encode();
        assert!(Frame::decode(&bytes[..12]).is_err(), "cut off");
        assert!(Frame::decode(&[0; MAX_NPDU + 1]).is_err(), "too long");
        let mut bad = bytes.clone();
        bad[0] = 0x09;
        assert!(Frame::decode(&bad).is_err(), "a command frame");
        let mut bad = bytes;
        bad[16] = 0x03;
        assert!(Frame::decode(&bad).is_err(), "fragmentation code");
    }
}
