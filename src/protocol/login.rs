use crate::MssqlConnectOptions;

use super::packet::{encode_message, PacketFrameError, PacketType};
use thiserror::Error;

const LOGIN7_FIXED_LEN: usize = 94;
const TDS_VERSION_74: u32 = 0x7400_0004;

const OPTION_FLAGS_1: u8 = 0xe0;
const OPTION_FLAGS_2: u8 = 0x03;
const TYPE_FLAGS: u8 = 0x00;
const OPTION_FLAGS_3: u8 = 0x00;

/// Builds an unframed TDS LOGIN7 payload from connection options.
pub fn build_login7_payload(options: &MssqlConnectOptions) -> Result<Vec<u8>, Login7Error> {
    let mut fields = Login7Fields::new(LOGIN7_FIXED_LEN);

    let hostname = fields.push_text(options.hostname(), false)?;
    let username = fields.push_text(options.username(), false)?;
    let password = fields.push_text(options.password().unwrap_or_default(), true)?;
    let app_name = fields.push_text(options.app_name(), false)?;
    let server_name = fields.push_text(options.server_name(), false)?;
    let unused = Login7FieldOffset::empty(fields.next_offset);
    let client_interface_name = fields.push_text(options.client_interface_name(), false)?;
    let language = fields.push_text(options.language(), false)?;
    let database = fields.push_text(options.database(), false)?;
    let sspi = Login7FieldOffset::empty(fields.next_offset);
    let attach_db_file = Login7FieldOffset::empty(fields.next_offset);
    let change_password = Login7FieldOffset::empty(fields.next_offset);

    let total_len = u32::from(fields.next_offset);
    let mut out = Vec::with_capacity(usize::from(fields.next_offset));

    write_u32_le(&mut out, total_len);
    write_u32_le(&mut out, TDS_VERSION_74);
    write_u32_le(&mut out, options.requested_packet_size());
    write_u32_le(&mut out, options.client_program_version());
    write_u32_le(&mut out, options.client_pid());
    write_u32_le(&mut out, 0);
    out.extend_from_slice(&[OPTION_FLAGS_1, OPTION_FLAGS_2, TYPE_FLAGS, OPTION_FLAGS_3]);
    write_i32_le(&mut out, 0);
    write_u32_le(&mut out, 0);

    for offset in [
        hostname,
        username,
        password,
        app_name,
        server_name,
        unused,
        client_interface_name,
        language,
        database,
    ] {
        offset.write_to(&mut out);
    }

    out.extend_from_slice(&[0; 6]);
    sspi.write_to(&mut out);
    attach_db_file.write_to(&mut out);
    change_password.write_to(&mut out);
    write_u32_le(&mut out, 0);

    debug_assert_eq!(LOGIN7_FIXED_LEN, out.len());
    out.extend_from_slice(&fields.data);

    Ok(out)
}

/// Builds framed TDS LOGIN7 packet bytes from connection options.
pub fn build_login7_packet(options: &MssqlConnectOptions) -> Result<Vec<u8>, Login7Error> {
    let payload = build_login7_payload(options)?;

    encode_message(
        PacketType::LOGIN7,
        &payload,
        usize::try_from(options.requested_packet_size())
            .map_err(|_| Login7Error::MessageTooLarge)?,
    )
    .map_err(Login7Error::Packet)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Login7FieldOffset {
    offset: u16,
    len_chars: u16,
}

impl Login7FieldOffset {
    fn empty(offset: u16) -> Self {
        Self {
            offset,
            len_chars: 0,
        }
    }

    fn write_to(self, out: &mut Vec<u8>) {
        write_u16_le(out, self.offset);
        write_u16_le(out, self.len_chars);
    }
}

struct Login7Fields {
    data: Vec<u8>,
    next_offset: u16,
}

impl Login7Fields {
    fn new(base_offset: usize) -> Self {
        Self {
            data: Vec::new(),
            next_offset: u16::try_from(base_offset).expect("LOGIN7 fixed header fits in u16"),
        }
    }

    fn push_text(
        &mut self,
        value: &str,
        obfuscate: bool,
    ) -> Result<Login7FieldOffset, Login7Error> {
        let offset = self.next_offset;
        let len_chars =
            u16::try_from(value.encode_utf16().count()).map_err(|_| Login7Error::FieldTooLong)?;
        let encoded = encode_utf16_le(value, obfuscate);
        let encoded_len = u16::try_from(encoded.len()).map_err(|_| Login7Error::MessageTooLarge)?;

        self.next_offset = self
            .next_offset
            .checked_add(encoded_len)
            .ok_or(Login7Error::MessageTooLarge)?;
        self.data.extend_from_slice(&encoded);

        Ok(Login7FieldOffset { offset, len_chars })
    }
}

fn encode_utf16_le(value: &str, obfuscate: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len() * 2);

    for unit in value.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }

    if obfuscate {
        for byte in &mut out {
            *byte = byte.rotate_left(4) ^ 0xa5;
        }
    }

    out
}

fn write_u16_le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_i32_le(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Error returned while building a LOGIN7 packet.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Login7Error {
    /// A text field exceeds the 16-bit LOGIN7 character-count field.
    #[error("TDS LOGIN7 text field is too long")]
    FieldTooLong,
    /// The payload cannot fit in LOGIN7's 16-bit offset fields.
    #[error("TDS LOGIN7 message is too large")]
    MessageTooLarge,
    /// Packet framing failed.
    #[error(transparent)]
    Packet(#[from] PacketFrameError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::packet::{PacketHeader, PacketStatus, PACKET_HEADER_LEN};

    #[test]
    fn builds_login7_payload_with_little_endian_fixed_fields() {
        let options = MssqlConnectOptions::parse_url(
            "mssql://alice:secret@example.com/appdb?packet_size=512&client_program_version=42&client_pid=7",
        )
        .unwrap();

        let payload = build_login7_payload(&options).unwrap();

        assert_eq!(
            payload.len() as u32,
            u32::from_le_bytes(payload[0..4].try_into().unwrap())
        );
        assert_eq!(
            TDS_VERSION_74,
            u32::from_le_bytes(payload[4..8].try_into().unwrap())
        );
        assert_eq!(512, u32::from_le_bytes(payload[8..12].try_into().unwrap()));
        assert_eq!(42, u32::from_le_bytes(payload[12..16].try_into().unwrap()));
        assert_eq!(7, u32::from_le_bytes(payload[16..20].try_into().unwrap()));
        assert_eq!(
            [OPTION_FLAGS_1, OPTION_FLAGS_2, TYPE_FLAGS, OPTION_FLAGS_3],
            payload[24..28]
        );
    }

    #[test]
    fn encodes_variable_fields_as_utf16_with_character_lengths() {
        let options = MssqlConnectOptions::parse_url(
            "mssql://al:pw@example.com/db?hostname=client&app_name=sqlx",
        )
        .unwrap();
        let payload = build_login7_payload(&options).unwrap();

        let hostname = field_at(&payload, 36);
        let username = field_at(&payload, 40);
        let password = field_at(&payload, 44);
        let app_name = field_at(&payload, 48);
        let database = field_at(&payload, 68);

        assert_eq!((94, 6), hostname);
        assert_eq!(b"c\0l\0i\0e\0n\0t\0", field_bytes(&payload, hostname));
        assert_eq!((106, 2), username);
        assert_eq!(b"a\0l\0", field_bytes(&payload, username));
        assert_eq!((114, 4), app_name);
        assert_eq!(b"s\0q\0l\0x\0", field_bytes(&payload, app_name));
        assert_eq!((122, 2), database);
        assert_eq!(b"d\0b\0", field_bytes(&payload, database));

        let raw_password = encode_utf16_le("pw", true);
        assert_eq!((110, 2), password);
        assert_eq!(raw_password.as_slice(), field_bytes(&payload, password));
        assert_ne!(b"p\0w\0", field_bytes(&payload, password));
    }

    #[test]
    fn frames_login7_payload_as_login7_packet() {
        let options = MssqlConnectOptions::parse_url(
            "mssql://alice:secret@example.com/master?packet_size=512",
        )
        .unwrap();
        let packet = build_login7_packet(&options).unwrap();
        let header = PacketHeader::decode(&packet[..PACKET_HEADER_LEN]).unwrap();

        assert_eq!(PacketType::LOGIN7, header.packet_type);
        assert_eq!(PacketStatus::END_OF_MESSAGE, header.status);
        assert_eq!(packet.len(), usize::from(header.length));
        assert_eq!(
            packet.len() - PACKET_HEADER_LEN,
            u32::from_le_bytes(
                packet[PACKET_HEADER_LEN..PACKET_HEADER_LEN + 4]
                    .try_into()
                    .unwrap()
            ) as usize
        );
    }

    #[test]
    fn rejects_text_fields_that_do_not_fit_login7_lengths() {
        let mut options = MssqlConnectOptions::new();
        options.set_hostname_for_test("a".repeat(usize::from(u16::MAX) + 1));

        let err = build_login7_payload(&options).unwrap_err();

        assert_eq!(Login7Error::FieldTooLong, err);
    }

    fn field_at(payload: &[u8], offset: usize) -> (usize, usize) {
        let start = usize::from(u16::from_le_bytes(
            payload[offset..offset + 2].try_into().unwrap(),
        ));
        let len_chars = usize::from(u16::from_le_bytes(
            payload[offset + 2..offset + 4].try_into().unwrap(),
        ));

        (start, len_chars)
    }

    fn field_bytes(payload: &[u8], field: (usize, usize)) -> &[u8] {
        &payload[field.0..field.0 + field.1 * 2]
    }
}
