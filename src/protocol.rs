use std::io::{self, Read, Write};
use thiserror::Error;

#[derive(Debug, PartialEq)]
pub enum Frame {
    Chat { id: u32, text: Vec<u8> },
    Dropped { id: u32 },
    System { text: Vec<u8> },
}

#[repr(u8)]
enum FrameType {
    Chat = 0,
    Dropped = 1,
    System = 2,
}

impl TryFrom<u8> for FrameType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(FrameType::Chat),
            1 => Ok(FrameType::Dropped),
            2 => Ok(FrameType::System),
            _ => Err(ProtocolError::UnknownFrameType(value)),
        }
    }
}
const MAX_PAYLOAD_SIZE: u16 = u16::MAX;
const ID_SIZE_IN_BYTES: usize = 4;
const HEADER_SIZE_IN_BYTES: usize = 3;

pub fn encode(frame: &Frame, stream: &mut impl Write) -> Result<(), ProtocolError> {
    let bytes = frame_to_bytes(frame)?;
    stream.write_all(&bytes)?;
    Ok(())
}

pub fn decode(stream: &mut impl Read) -> Result<Frame, ProtocolError> {
    let mut header_buf = [0u8; 3];

    match stream.read_exact(&mut header_buf) {
        Ok(_) => {}
        Err(e) => {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                return Err(ProtocolError::Disconnect);
            }
            return Err(ProtocolError::IO(e));
        }
    };

    let frame_type = FrameType::try_from(header_buf[0])?;
    let len = u16::from_be_bytes(header_buf[1..].try_into().unwrap());

    let mut payload = vec![0u8; len as usize];

    match stream.read_exact(&mut payload) {
        Ok(_) => post_read_parse(frame_type, payload),
        Err(e) => {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                return Err(ProtocolError::Disconnect);
            }
            Err(ProtocolError::IO(e))
        }
    }
}

fn extract_id(payload: &[u8]) -> Result<u32, ProtocolError> {
    let id_payload_part = payload
        .first_chunk::<4>()
        .ok_or(ProtocolError::PayloadIsMalformed)?;
    Ok(u32::from_be_bytes(*id_payload_part))
}

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("payload is too large: {0}")]
    PayloadIsTooLong(usize),
    #[error("couldn't write the frame")]
    IO(#[from] io::Error),
    #[error("disconnect")]
    Disconnect,
    #[error("Unknown frame type {0}")]
    UnknownFrameType(u8),
    #[error("Payload is malformed or too short")]
    PayloadIsMalformed,
}

fn frame_to_bytes(frame: &Frame) -> Result<Vec<u8>, ProtocolError> {
    let (type_byte, payload_len) = compute_prealloc_capacity_for_frame(frame);

    if payload_len > MAX_PAYLOAD_SIZE as usize {
        return Err(ProtocolError::PayloadIsTooLong(payload_len));
    }

    let mut result: Vec<u8> = Vec::with_capacity(HEADER_SIZE_IN_BYTES + payload_len);
    result.push(type_byte);
    result.extend_from_slice(&((payload_len as u16).to_be_bytes()));

    match frame {
        Frame::Chat { id, text } => {
            result.extend_from_slice(&id.to_be_bytes());
            result.extend_from_slice(text);
        }
        Frame::System { text } => result.extend_from_slice(text),
        Frame::Dropped { id } => result.extend_from_slice(&id.to_be_bytes()),
    }
    Ok(result)
}

fn compute_prealloc_capacity_for_frame(frame: &Frame) -> (u8, usize) {
    match frame {
        Frame::Chat { text, .. } => (FrameType::Chat as u8, ID_SIZE_IN_BYTES + text.len()),
        Frame::System { text } => (FrameType::System as u8, text.len()),
        Frame::Dropped { .. } => (FrameType::Dropped as u8, ID_SIZE_IN_BYTES),
    }
}

fn post_read_parse(frame_type: FrameType, payload: Vec<u8>) -> Result<Frame, ProtocolError> {
    match frame_type {
        FrameType::Chat => {
            let id = extract_id(&payload)?;
            let (_, text_payload_part) = payload.split_first_chunk::<4>().unwrap();
            let text = Vec::from(text_payload_part);
            Ok(Frame::Chat { id, text })
        }
        FrameType::Dropped => {
            let id = extract_id(&payload)?;
            Ok(Frame::Dropped { id })
        }
        FrameType::System => Ok(Frame::System { text: payload }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (`Vec<u8>` is a `Write` sink, `&[u8]` is a `Read` source). Proves encode/decode are inverses.
    fn roundtrip(frame: Frame) {
        let mut buf = Vec::new();
        encode(&frame, &mut buf).expect("encode should succeed");
        let decoded = decode(&mut buf.as_slice()).expect("decode should succeed");
        assert_eq!(
            decoded, frame,
            "frame did not survive an encode/decode round-trip"
        );
    }

    #[test]
    fn roundtrips_chat() {
        roundtrip(Frame::Chat {
            id: 7,
            text: b"hello".to_vec(),
        });
    }

    #[test]
    fn roundtrips_system() {
        roundtrip(Frame::System {
            text: b"welcome to the club".to_vec(),
        });
    }

    #[test]
    fn roundtrips_dropped() {
        roundtrip(Frame::Dropped { id: 42 });
    }

    #[test]
    fn roundtrips_empty_text() {
        // empty payloads are a classic off-by-one framing trap
        roundtrip(Frame::Chat {
            id: 0,
            text: Vec::new(),
        });
        roundtrip(Frame::System { text: Vec::new() });
    }

    // Golden bytes: pin the EXACT wire format so a refactor can't silently change it.
    // Round-trip alone can't catch a symmetric encode+decode mistake; this can.
    #[test]
    fn golden_bytes_system() {
        let mut buf = Vec::new();
        encode(
            &Frame::System {
                text: b"hi".to_vec(),
            },
            &mut buf,
        )
        .unwrap();
        // [type=2][len=0,2 (u16 BE)][payload 'h','i']
        assert_eq!(buf, vec![2, 0, 2, b'h', b'i']);
    }

    #[test]
    fn golden_bytes_chat() {
        let mut buf = Vec::new();
        encode(
            &Frame::Chat {
                id: 7,
                text: b"hi".to_vec(),
            },
            &mut buf,
        )
        .unwrap();
        // [type=0][len=0,6][id=0,0,0,7 (u32 BE)][payload 'h','i']
        assert_eq!(buf, vec![0, 0, 6, 0, 0, 0, 7, b'h', b'i']);
    }

    #[test]
    fn decode_rejects_unknown_frame_type() {
        let bytes = [9u8, 0, 0]; // type byte 9 is not a known FrameType, len 0
        let result = decode(&mut &bytes[..]);
        assert!(matches!(result, Err(ProtocolError::UnknownFrameType(9))));
    }

    #[test]
    fn decode_rejects_short_chat_payload() {
        // type=Chat(0), len=2, but a Chat needs >=4 payload bytes for its id
        let bytes = [0u8, 0, 2, 1, 2];
        let result = decode(&mut &bytes[..]);
        assert!(matches!(result, Err(ProtocolError::PayloadIsMalformed)));
    }
}
