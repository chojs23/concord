//! Discord RPC/IPC wire framing: an 8-byte little-endian header then a JSON body.
//!
//! ```text
//! [opcode: u32 LE][length: u32 LE][body: `length` bytes]
//! ```

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Caps allocation driven by an attacker-controlled length header.
const MAX_FRAME_LEN: u32 = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Opcode {
    Handshake,
    Frame,
    Close,
    Ping,
    Pong,
}

impl Opcode {
    fn to_u32(self) -> u32 {
        match self {
            Self::Handshake => 0,
            Self::Frame => 1,
            Self::Close => 2,
            Self::Ping => 3,
            Self::Pong => 4,
        }
    }

    fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Handshake),
            1 => Some(Self::Frame),
            2 => Some(Self::Close),
            3 => Some(Self::Ping),
            4 => Some(Self::Pong),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Frame {
    pub opcode: Opcode,
    pub payload: Vec<u8>,
}

pub(super) fn encode_frame(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(8 + payload.len());
    buffer.extend_from_slice(&opcode.to_u32().to_le_bytes());
    buffer.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buffer.extend_from_slice(payload);
    buffer
}

pub(super) async fn write_frame<W>(writer: &mut W, opcode: Opcode, payload: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&encode_frame(opcode, payload)).await?;
    writer.flush().await
}

pub(super) async fn read_frame<R>(reader: &mut R) -> io::Result<Frame>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 8];
    reader.read_exact(&mut header).await?;
    let opcode_raw = u32::from_le_bytes(header[0..4].try_into().expect("4-byte opcode slice"));
    let length = u32::from_le_bytes(header[4..8].try_into().expect("4-byte length slice"));

    let opcode = Opcode::from_u32(opcode_raw).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown RPC opcode {opcode_raw}"),
        )
    })?;
    if length > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("RPC frame too large: {length} bytes"),
        ));
    }

    let mut payload = vec![0u8; length as usize];
    reader.read_exact(&mut payload).await?;
    Ok(Frame { opcode, payload })
}

#[cfg(test)]
mod tests {
    use super::{Frame, Opcode, encode_frame, read_frame};

    #[test]
    fn encode_frame_uses_little_endian_opcode_and_length_header() {
        let bytes = encode_frame(Opcode::Frame, b"hi");
        assert_eq!(&bytes[0..4], &1u32.to_le_bytes());
        assert_eq!(&bytes[4..8], &2u32.to_le_bytes());
        assert_eq!(&bytes[8..], b"hi");
    }

    #[tokio::test]
    async fn read_frame_round_trips_encoded_frame() {
        let encoded = encode_frame(Opcode::Handshake, br#"{"v":1}"#);
        let mut cursor = encoded.as_slice();
        let frame = read_frame(&mut cursor)
            .await
            .expect("encoded frame should decode");
        assert_eq!(
            frame,
            Frame {
                opcode: Opcode::Handshake,
                payload: br#"{"v":1}"#.to_vec(),
            }
        );
    }
}
