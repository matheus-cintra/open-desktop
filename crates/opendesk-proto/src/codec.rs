use crate::control::ControlMessage;
use crate::input::InputDatagram;

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_DATAGRAM_BYTES: usize = 512;
const LENGTH_PREFIX_BYTES: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("frame of {0} bytes exceeds the {MAX_FRAME_BYTES} byte limit")]
    FrameTooLarge(usize),
    #[error("serialization failed: {0}")]
    Serialize(postcard::Error),
    #[error("deserialization failed: {0}")]
    Deserialize(postcard::Error),
}

pub fn encode_frame(message: &ControlMessage) -> Result<Vec<u8>, CodecError> {
    let payload = postcard::to_stdvec(message).map_err(CodecError::Serialize)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge(payload.len()));
    }
    let length =
        u32::try_from(payload.len()).map_err(|_| CodecError::FrameTooLarge(payload.len()))?;
    let mut frame = Vec::with_capacity(LENGTH_PREFIX_BYTES + payload.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[derive(Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    consumed: usize,
}

impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) {
        if self.consumed > 0 {
            self.buffer.drain(..self.consumed);
            self.consumed = 0;
        }
        self.buffer.extend_from_slice(bytes);
    }

    pub fn next_frame(&mut self) -> Result<Option<ControlMessage>, CodecError> {
        let available = self.buffer.get(self.consumed..).unwrap_or_default();
        let Some(prefix) = available.first_chunk::<LENGTH_PREFIX_BYTES>() else {
            return Ok(None);
        };
        let length = u32::from_be_bytes(*prefix) as usize;
        if length > MAX_FRAME_BYTES {
            return Err(CodecError::FrameTooLarge(length));
        }
        let Some(payload) = available.get(LENGTH_PREFIX_BYTES..LENGTH_PREFIX_BYTES + length) else {
            return Ok(None);
        };
        let message = postcard::from_bytes(payload).map_err(CodecError::Deserialize)?;
        self.consumed += LENGTH_PREFIX_BYTES + length;
        Ok(Some(message))
    }
}

pub fn encode_datagram(datagram: &InputDatagram) -> Result<Vec<u8>, CodecError> {
    postcard::to_stdvec(datagram).map_err(CodecError::Serialize)
}

pub fn decode_datagram(bytes: &[u8]) -> Result<InputDatagram, CodecError> {
    postcard::from_bytes(bytes).map_err(CodecError::Deserialize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{PeerId, Side, Token};
    use crate::input::{Axis, AxisSource, InputEvent, InputHeader};

    fn sample_messages() -> Vec<ControlMessage> {
        vec![
            ControlMessage::Hello {
                protocol_version: 1,
                peer_id: PeerId([7; 16]),
                name: "desktop".to_owned(),
                token: Some(Token([9; 32])),
                layout: Vec::new(),
            },
            ControlMessage::RequestControl {
                side: Side::Left,
                fraction: 0.25,
                drag: None,
            },
            ControlMessage::Key {
                code: 30,
                pressed: true,
            },
            ControlMessage::ClipboardSet {
                mime: "text/plain".to_owned(),
                bytes: b"hi".to_vec(),
            },
        ]
    }

    #[test]
    fn frames_round_trip_one_at_a_time() {
        for message in sample_messages() {
            let mut decoder = FrameDecoder::default();
            decoder.push(&encode_frame(&message).unwrap());
            assert_eq!(decoder.next_frame().unwrap(), Some(message));
            assert_eq!(decoder.next_frame().unwrap(), None);
        }
    }

    #[test]
    fn frames_split_across_pushes_and_coalesced_in_one_push() {
        let messages = sample_messages();
        let mut stream = Vec::new();
        for message in &messages {
            stream.extend(encode_frame(message).unwrap());
        }
        let mut decoder = FrameDecoder::default();
        let (head, tail) = stream.split_at(7);
        decoder.push(head);
        assert_eq!(decoder.next_frame().unwrap(), None);
        decoder.push(tail);
        for message in &messages {
            assert_eq!(decoder.next_frame().unwrap().as_ref(), Some(message));
        }
        assert_eq!(decoder.next_frame().unwrap(), None);
    }

    #[test]
    fn oversized_length_prefix_is_rejected() {
        let mut decoder = FrameDecoder::default();
        decoder.push(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decoder.next_frame(),
            Err(CodecError::FrameTooLarge(_))
        ));
    }

    #[test]
    fn datagrams_round_trip_and_stay_small() {
        let datagram = InputDatagram {
            header: InputHeader {
                session_id: 3,
                sequence: 99,
            },
            event: InputEvent::Axis {
                axis: Axis::Vertical,
                value: -15.0,
                value120: -120,
                source: AxisSource::Wheel,
            },
        };
        let bytes = encode_datagram(&datagram).unwrap();
        assert!(bytes.len() <= MAX_DATAGRAM_BYTES);
        assert_eq!(decode_datagram(&bytes).unwrap(), datagram);
    }
}
