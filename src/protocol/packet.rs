use thiserror::Error;

/// Length in bytes of a TDS packet header.
pub const PACKET_HEADER_LEN: usize = 8;

/// Maximum encoded TDS packet length. The packet header stores this as a u16.
pub const MAX_PACKET_LEN: usize = u16::MAX as usize;

/// TDS packet type byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketType(u8);

impl PacketType {
    /// SQL batch packet.
    pub const SQL_BATCH: Self = Self(0x01);
    /// RPC request packet.
    pub const RPC: Self = Self(0x03);
    /// Tabular result packet.
    pub const TABULAR_RESULT: Self = Self(0x04);
    /// Login7 packet.
    pub const LOGIN7: Self = Self(0x10);
    /// Pre-login packet.
    pub const PRE_LOGIN: Self = Self(0x12);

    /// Returns the raw packet type byte.
    pub const fn code(self) -> u8 {
        self.0
    }
}

impl From<u8> for PacketType {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

/// TDS packet status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketStatus(u8);

impl PacketStatus {
    /// Normal packet.
    pub const NORMAL: Self = Self(0x00);
    /// Last packet in a message.
    pub const END_OF_MESSAGE: Self = Self(0x01);

    /// Returns the raw status byte.
    pub const fn code(self) -> u8 {
        self.0
    }
}

impl From<u8> for PacketStatus {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

/// TDS packet header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    /// Packet type.
    pub packet_type: PacketType,
    /// Packet status.
    pub status: PacketStatus,
    /// Full packet length including the 8-byte header.
    pub length: u16,
    /// Server process ID.
    pub server_process_id: u16,
    /// Packet sequence ID.
    pub packet_id: u8,
    /// TDS window byte. Usually zero.
    pub window: u8,
}

impl PacketHeader {
    /// Creates a packet header for an outgoing client packet.
    pub fn new(packet_type: PacketType, status: PacketStatus, length: u16, packet_id: u8) -> Self {
        Self {
            packet_type,
            status,
            length,
            server_process_id: 0,
            packet_id,
            window: 0,
        }
    }

    /// Encodes this header to its wire representation.
    pub fn encode(self) -> [u8; PACKET_HEADER_LEN] {
        let length = self.length.to_be_bytes();
        let server_process_id = self.server_process_id.to_be_bytes();

        [
            self.packet_type.code(),
            self.status.code(),
            length[0],
            length[1],
            server_process_id[0],
            server_process_id[1],
            self.packet_id,
            self.window,
        ]
    }

    /// Decodes a TDS packet header from its wire representation.
    pub fn decode(input: &[u8]) -> Result<Self, PacketHeaderError> {
        let bytes: &[u8; PACKET_HEADER_LEN] = input
            .try_into()
            .map_err(|_| PacketHeaderError::WrongLength(input.len()))?;

        let length = u16::from_be_bytes([bytes[2], bytes[3]]);

        if usize::from(length) < PACKET_HEADER_LEN {
            return Err(PacketHeaderError::InvalidPacketLength(length));
        }

        Ok(Self {
            packet_type: PacketType::from(bytes[0]),
            status: PacketStatus::from(bytes[1]),
            length,
            server_process_id: u16::from_be_bytes([bytes[4], bytes[5]]),
            packet_id: bytes[6],
            window: bytes[7],
        })
    }
}

/// A decoded TDS message assembled from one or more packets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketMessage {
    /// Packet type shared by all packets in the message.
    pub packet_type: PacketType,
    /// Concatenated message payload, excluding packet headers.
    pub payload: Vec<u8>,
    /// Number of bytes consumed from the input buffer.
    pub consumed: usize,
}

/// Encodes a message payload into one or more TDS packets.
///
/// `packet_size` is the maximum packet length including the 8-byte header. The
/// helper emits client packet IDs starting at one and sets
/// `END_OF_MESSAGE` only on the final packet.
pub fn encode_message(
    packet_type: PacketType,
    payload: &[u8],
    packet_size: usize,
) -> Result<Vec<u8>, PacketFrameError> {
    if packet_size <= PACKET_HEADER_LEN {
        return Err(PacketFrameError::InvalidMaxPacketSize(packet_size));
    }

    if packet_size > MAX_PACKET_LEN {
        return Err(PacketFrameError::InvalidMaxPacketSize(packet_size));
    }

    let max_payload_len = packet_size - PACKET_HEADER_LEN;
    let packet_count = if payload.is_empty() {
        1
    } else {
        payload.len().div_ceil(max_payload_len)
    };

    let total_len = payload
        .len()
        .checked_add(packet_count * PACKET_HEADER_LEN)
        .ok_or(PacketFrameError::MessageTooLarge)?;

    let mut out = Vec::with_capacity(total_len);
    let mut packet_id = 1u8;

    if payload.is_empty() {
        let header = PacketHeader::new(
            packet_type,
            PacketStatus::END_OF_MESSAGE,
            PACKET_HEADER_LEN as u16,
            packet_id,
        );
        out.extend_from_slice(&header.encode());
        return Ok(out);
    }

    for chunk in payload.chunks(max_payload_len) {
        let is_last = out.len() + PACKET_HEADER_LEN + chunk.len() == total_len;
        let status = if is_last {
            PacketStatus::END_OF_MESSAGE
        } else {
            PacketStatus::NORMAL
        };
        let length = u16::try_from(PACKET_HEADER_LEN + chunk.len())
            .map_err(|_| PacketFrameError::MessageTooLarge)?;

        let header = PacketHeader::new(packet_type, status, length, packet_id);
        out.extend_from_slice(&header.encode());
        out.extend_from_slice(chunk);
        packet_id = packet_id.wrapping_add(1);
    }

    Ok(out)
}

/// Tries to decode one complete TDS message from the front of `input`.
///
/// Returns `Ok(None)` when the buffer does not yet contain a full packet or a
/// packet marked `END_OF_MESSAGE`. On success, `PacketMessage::consumed`
/// identifies how many bytes can be removed from the caller's receive buffer.
pub fn try_decode_message(input: &[u8]) -> Result<Option<PacketMessage>, PacketFrameError> {
    let mut offset = 0usize;
    let mut packet_type = None;
    let mut expected_packet_id = None;
    let mut payload = Vec::new();

    loop {
        let Some(header_bytes) = input.get(offset..offset + PACKET_HEADER_LEN) else {
            return Ok(None);
        };

        let header = PacketHeader::decode(header_bytes)?;

        if let Some(packet_type) = packet_type {
            if header.packet_type != packet_type {
                return Err(PacketFrameError::MismatchedPacketType {
                    expected: packet_type,
                    actual: header.packet_type,
                });
            }
        } else {
            packet_type = Some(header.packet_type);
        }

        if let Some(packet_id) = expected_packet_id {
            if header.packet_id != packet_id {
                return Err(PacketFrameError::UnexpectedPacketId {
                    expected: packet_id,
                    actual: header.packet_id,
                });
            }
        }

        let packet_len = usize::from(header.length);
        let packet_end = offset + packet_len;
        let Some(packet) = input.get(offset + PACKET_HEADER_LEN..packet_end) else {
            return Ok(None);
        };

        payload
            .try_reserve(packet.len())
            .map_err(|_| PacketFrameError::MessageTooLarge)?;
        payload.extend_from_slice(packet);
        offset = packet_end;
        expected_packet_id = Some(header.packet_id.wrapping_add(1));

        if header.status == PacketStatus::END_OF_MESSAGE {
            return Ok(Some(PacketMessage {
                packet_type: packet_type.expect("packet_type is set after decoding a header"),
                payload,
                consumed: offset,
            }));
        }
    }
}

/// Error returned while decoding a TDS packet header.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PacketHeaderError {
    /// The header input did not contain exactly 8 bytes.
    #[error("TDS packet header must be 8 bytes, got {0}")]
    WrongLength(usize),
    /// The encoded packet length is smaller than the header itself.
    #[error("TDS packet length {0} is smaller than the 8-byte header")]
    InvalidPacketLength(u16),
}

/// Error returned while framing or deframing TDS packets.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PacketFrameError {
    /// Packet header decoding failed.
    #[error(transparent)]
    Header(#[from] PacketHeaderError),
    /// The requested packet size cannot be encoded in a TDS packet header or
    /// leaves no room for payload bytes.
    #[error("invalid maximum TDS packet size {0}")]
    InvalidMaxPacketSize(usize),
    /// A decoded message contained packets with different packet types.
    #[error("TDS message packet type changed from 0x{expected:02x} to 0x{actual:02x}")]
    MismatchedPacketType {
        /// Packet type from the first packet.
        expected: PacketType,
        /// Packet type from a later packet in the same message.
        actual: PacketType,
    },
    /// Packet IDs in a multi-packet message were not contiguous.
    #[error("unexpected TDS packet id {actual}, expected {expected}")]
    UnexpectedPacketId {
        /// Expected packet ID.
        expected: u8,
        /// Packet ID from the header.
        actual: u8,
    },
    /// The message could not fit in memory or in a protocol length field.
    #[error("TDS message is too large")]
    MessageTooLarge,
}

impl std::fmt::LowerHex for PacketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::LowerHex::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_header_with_big_endian_integer_fields() {
        let header = PacketHeader {
            packet_type: PacketType::PRE_LOGIN,
            status: PacketStatus::END_OF_MESSAGE,
            length: 0x1234,
            server_process_id: 0xabcd,
            packet_id: 7,
            window: 0,
        };

        assert_eq!(
            [0x12, 0x01, 0x12, 0x34, 0xab, 0xcd, 0x07, 0x00],
            header.encode()
        );
    }

    #[test]
    fn decodes_header_from_wire_bytes() {
        let header =
            PacketHeader::decode(&[0x04, 0x01, 0x00, 0x08, 0x00, 0x2a, 0x03, 0x00]).unwrap();

        assert_eq!(PacketType::TABULAR_RESULT, header.packet_type);
        assert_eq!(PacketStatus::END_OF_MESSAGE, header.status);
        assert_eq!(8, header.length);
        assert_eq!(42, header.server_process_id);
        assert_eq!(3, header.packet_id);
    }

    #[test]
    fn rejects_header_with_impossible_length() {
        let err = PacketHeader::decode(&[0x12, 0x01, 0x00, 0x07, 0, 0, 0, 0]).unwrap_err();

        assert_eq!(PacketHeaderError::InvalidPacketLength(7), err);
    }

    #[test]
    fn encodes_empty_message_as_end_packet() {
        let bytes = encode_message(PacketType::SQL_BATCH, &[], 512).unwrap();

        assert_eq!(vec![0x01, 0x01, 0x00, 0x08, 0, 0, 1, 0], bytes);
    }

    #[test]
    fn encodes_client_message_across_packet_boundaries_from_packet_id_one() {
        let bytes = encode_message(PacketType::PRE_LOGIN, b"abcdefghi", 12).unwrap();

        assert_eq!(
            vec![
                0x12, 0x00, 0x00, 0x0c, 0, 0, 1, 0, b'a', b'b', b'c', b'd', 0x12, 0x00, 0x00, 0x0c,
                0, 0, 2, 0, b'e', b'f', b'g', b'h', 0x12, 0x01, 0x00, 0x09, 0, 0, 3, 0, b'i',
            ],
            bytes
        );
    }

    #[test]
    fn rejects_invalid_max_packet_size() {
        let err = encode_message(PacketType::PRE_LOGIN, b"abc", PACKET_HEADER_LEN).unwrap_err();

        assert_eq!(
            PacketFrameError::InvalidMaxPacketSize(PACKET_HEADER_LEN),
            err
        );
    }

    #[test]
    fn decodes_single_packet_message_and_reports_consumed_bytes() {
        let mut bytes = encode_message(PacketType::SQL_BATCH, b"select 1", 512).unwrap();
        bytes.extend_from_slice(b"next message bytes");

        let message = try_decode_message(&bytes).unwrap().unwrap();

        assert_eq!(PacketType::SQL_BATCH, message.packet_type);
        assert_eq!(b"select 1", message.payload.as_slice());
        assert_eq!(PACKET_HEADER_LEN + b"select 1".len(), message.consumed);
    }

    #[test]
    fn decodes_multi_packet_message_payload() {
        let bytes = contiguous_packet_id_message();
        let message = try_decode_message(&bytes).unwrap().unwrap();

        assert_eq!(PacketType::PRE_LOGIN, message.packet_type);
        assert_eq!(b"abcdefghi", message.payload.as_slice());
        assert_eq!(bytes.len(), message.consumed);
    }

    #[test]
    fn waits_for_complete_packet() {
        let bytes = contiguous_packet_id_message();

        assert_eq!(None, try_decode_message(&bytes[..15]).unwrap());
    }

    #[test]
    fn waits_for_end_of_message_packet() {
        let bytes = contiguous_packet_id_message();

        assert_eq!(None, try_decode_message(&bytes[..12]).unwrap());
    }

    #[test]
    fn rejects_mismatched_packet_types() {
        let mut bytes = contiguous_packet_id_message();
        bytes[12] = PacketType::SQL_BATCH.code();

        let err = try_decode_message(&bytes).unwrap_err();

        assert_eq!(
            PacketFrameError::MismatchedPacketType {
                expected: PacketType::PRE_LOGIN,
                actual: PacketType::SQL_BATCH,
            },
            err
        );
    }

    #[test]
    fn rejects_non_contiguous_packet_ids() {
        let mut bytes = contiguous_packet_id_message();
        bytes[18] = 5;

        let err = try_decode_message(&bytes).unwrap_err();

        assert_eq!(
            PacketFrameError::UnexpectedPacketId {
                expected: 2,
                actual: 5,
            },
            err
        );
    }

    fn contiguous_packet_id_message() -> Vec<u8> {
        vec![
            0x12, 0x00, 0x00, 0x0c, 0, 0, 1, 0, b'a', b'b', b'c', b'd', 0x12, 0x00, 0x00, 0x0c, 0,
            0, 2, 0, b'e', b'f', b'g', b'h', 0x12, 0x01, 0x00, 0x09, 0, 0, 3, 0, b'i',
        ]
    }
}
