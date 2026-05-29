use crate::{Encrypt, MssqlConnectOptions};
use thiserror::Error;

use super::packet::{encode_message, PacketFrameError, PacketType};

/// TDS pre-login option token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PreLoginOptionToken {
    /// Protocol version.
    Version = 0x00,
    /// Encryption negotiation.
    Encryption = 0x01,
    /// Instance name.
    Instance = 0x02,
    /// Thread ID.
    ThreadId = 0x03,
    /// Multiple active result sets flag.
    Mars = 0x04,
    /// Option-list terminator.
    Terminator = 0xff,
}

impl TryFrom<u8> for PreLoginOptionToken {
    type Error = PreLoginError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::Version),
            0x01 => Ok(Self::Encryption),
            0x02 => Ok(Self::Instance),
            0x03 => Ok(Self::ThreadId),
            0x04 => Ok(Self::Mars),
            0xff => Ok(Self::Terminator),
            _ => Err(PreLoginError::UnknownToken(value)),
        }
    }
}

impl From<PreLoginOptionToken> for u8 {
    fn from(value: PreLoginOptionToken) -> Self {
        value as u8
    }
}

/// One pre-login option and its raw payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreLoginOption {
    /// Option token.
    pub token: PreLoginOptionToken,
    /// Raw option payload.
    pub data: Vec<u8>,
}

/// Assembles a TDS pre-login option table and payload.
///
/// Each table entry is encoded as `token`, big-endian `offset`, and big-endian
/// `length`, followed by a `0xff` terminator and then the concatenated option
/// payloads. Offsets are relative to the beginning of the pre-login message.
pub fn assemble_options(options: &[PreLoginOption]) -> Result<Vec<u8>, PreLoginError> {
    let table_len = options
        .len()
        .checked_mul(5)
        .and_then(|len| len.checked_add(1))
        .ok_or(PreLoginError::MessageTooLarge)?;

    let mut offset = u16::try_from(table_len).map_err(|_| PreLoginError::MessageTooLarge)?;
    let payload_len = options
        .iter()
        .map(|option| option.data.len())
        .try_fold(0usize, |sum, len| {
            sum.checked_add(len).ok_or(PreLoginError::MessageTooLarge)
        })?;

    let total_len = table_len
        .checked_add(payload_len)
        .ok_or(PreLoginError::MessageTooLarge)?;

    u16::try_from(total_len).map_err(|_| PreLoginError::MessageTooLarge)?;

    let mut out = Vec::with_capacity(total_len);

    for option in options {
        if option.token == PreLoginOptionToken::Terminator {
            return Err(PreLoginError::TerminatorOption);
        }

        let len = u16::try_from(option.data.len()).map_err(|_| PreLoginError::MessageTooLarge)?;

        out.push(option.token.into());
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&len.to_be_bytes());

        offset = offset
            .checked_add(len)
            .ok_or(PreLoginError::MessageTooLarge)?;
    }

    out.push(PreLoginOptionToken::Terminator.into());

    for option in options {
        out.extend_from_slice(&option.data);
    }

    Ok(out)
}

/// Parses a TDS pre-login option table and payload.
pub fn parse_options(input: &[u8]) -> Result<Vec<PreLoginOption>, PreLoginError> {
    let terminator = input
        .iter()
        .position(|byte| *byte == u8::from(PreLoginOptionToken::Terminator))
        .ok_or(PreLoginError::MissingTerminator)?;

    if terminator % 5 != 0 {
        return Err(PreLoginError::TruncatedOptionTable);
    }

    let mut options = Vec::with_capacity(terminator / 5);

    for entry in input[..terminator].chunks_exact(5) {
        let token = PreLoginOptionToken::try_from(entry[0])?;
        let offset = usize::from(u16::from_be_bytes([entry[1], entry[2]]));
        let len = usize::from(u16::from_be_bytes([entry[3], entry[4]]));
        let end = offset
            .checked_add(len)
            .ok_or(PreLoginError::OptionOutOfBounds { offset, len })?;

        let data = input
            .get(offset..end)
            .ok_or(PreLoginError::OptionOutOfBounds { offset, len })?
            .to_vec();

        options.push(PreLoginOption { token, data });
    }

    Ok(options)
}

/// Maps SQL Server connection encryption preferences to the TDS pre-login byte.
pub fn encode_encrypt(encrypt: Encrypt) -> u8 {
    match encrypt {
        Encrypt::Off => 0x00,
        Encrypt::On => 0x01,
        Encrypt::NotSupported => 0x02,
        Encrypt::Required => 0x03,
    }
}

/// Maps a TDS pre-login encryption byte to a connection encryption preference.
pub fn decode_encrypt(value: u8) -> Result<Encrypt, PreLoginError> {
    match value {
        0x00 => Ok(Encrypt::Off),
        0x01 => Ok(Encrypt::On),
        0x02 => Ok(Encrypt::NotSupported),
        0x03 => Ok(Encrypt::Required),
        _ => Err(PreLoginError::InvalidEncrypt(value)),
    }
}

/// Builds an unframed TDS PRELOGIN payload from connection options.
pub fn build_pre_login_payload(options: &MssqlConnectOptions) -> Result<Vec<u8>, PreLoginError> {
    let mut pre_login_options = vec![
        PreLoginOption {
            token: PreLoginOptionToken::Version,
            data: vec![0, 0, 0, 0, 0, 0],
        },
        PreLoginOption {
            token: PreLoginOptionToken::Encryption,
            data: vec![encode_encrypt(options.encrypt())],
        },
        PreLoginOption {
            token: PreLoginOptionToken::Mars,
            data: vec![0],
        },
    ];

    if let Some(instance) = options.instance() {
        let mut data = instance.as_bytes().to_vec();
        data.push(0);
        pre_login_options.push(PreLoginOption {
            token: PreLoginOptionToken::Instance,
            data,
        });
    }

    assemble_options(&pre_login_options)
}

/// Builds framed TDS PRELOGIN packet bytes from connection options.
pub fn build_pre_login_packet(options: &MssqlConnectOptions) -> Result<Vec<u8>, PreLoginError> {
    let payload = build_pre_login_payload(options)?;

    encode_message(
        PacketType::PRE_LOGIN,
        &payload,
        usize::try_from(options.requested_packet_size())
            .map_err(|_| PreLoginError::MessageTooLarge)?,
    )
    .map_err(PreLoginError::Packet)
}

/// Extracts the server encryption response from a PRELOGIN payload.
pub fn parse_server_encrypt(input: &[u8]) -> Result<Encrypt, PreLoginError> {
    let options = parse_options(input)?;
    let encryption = options
        .iter()
        .find(|option| option.token == PreLoginOptionToken::Encryption)
        .and_then(|option| option.data.first().copied())
        .ok_or(PreLoginError::MissingEncryption)?;

    decode_encrypt(encryption)
}

/// Error returned while decoding a pre-login helper value.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreLoginError {
    /// The option token is not defined by this helper.
    #[error("unknown TDS pre-login option token 0x{0:02x}")]
    UnknownToken(u8),
    /// The encryption value is not defined by TDS.
    #[error("invalid TDS pre-login encryption value 0x{0:02x}")]
    InvalidEncrypt(u8),
    /// The option table did not include a terminator byte.
    #[error("TDS pre-login option table is missing its terminator")]
    MissingTerminator,
    /// The option table terminator appeared in the middle of an option entry.
    #[error("TDS pre-login option table is truncated")]
    TruncatedOptionTable,
    /// A regular pre-login option used the terminator token.
    #[error("TDS pre-login terminator cannot be encoded as an option")]
    TerminatorOption,
    /// An option offset and length point outside the message buffer.
    #[error("TDS pre-login option points outside the message: offset {offset}, length {len}")]
    OptionOutOfBounds {
        /// Option payload offset.
        offset: usize,
        /// Option payload length.
        len: usize,
    },
    /// The assembled pre-login message exceeds the protocol's 16-bit offsets.
    #[error("TDS pre-login message is too large")]
    MessageTooLarge,
    /// Packet framing failed.
    #[error(transparent)]
    Packet(#[from] PacketFrameError),
    /// The server PRELOGIN response did not include an encryption option.
    #[error("TDS pre-login response is missing its encryption option")]
    MissingEncryption,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_values_round_trip() {
        for encrypt in [
            Encrypt::NotSupported,
            Encrypt::Off,
            Encrypt::On,
            Encrypt::Required,
        ] {
            assert_eq!(encrypt, decode_encrypt(encode_encrypt(encrypt)).unwrap());
        }
    }

    #[test]
    fn encryption_values_match_tds_wire_values() {
        assert_eq!(0x00, encode_encrypt(Encrypt::Off));
        assert_eq!(0x01, encode_encrypt(Encrypt::On));
        assert_eq!(0x02, encode_encrypt(Encrypt::NotSupported));
        assert_eq!(0x03, encode_encrypt(Encrypt::Required));

        assert_eq!(Encrypt::Off, decode_encrypt(0x00).unwrap());
        assert_eq!(Encrypt::On, decode_encrypt(0x01).unwrap());
        assert_eq!(Encrypt::NotSupported, decode_encrypt(0x02).unwrap());
        assert_eq!(Encrypt::Required, decode_encrypt(0x03).unwrap());
    }

    #[test]
    fn rejects_unknown_encryption_value() {
        assert_eq!(
            Err(PreLoginError::InvalidEncrypt(0x7f)),
            decode_encrypt(0x7f)
        );
    }

    #[test]
    fn decodes_known_option_tokens() {
        assert_eq!(
            PreLoginOptionToken::Encryption,
            PreLoginOptionToken::try_from(0x01).unwrap()
        );
        assert_eq!(
            PreLoginOptionToken::Terminator,
            PreLoginOptionToken::try_from(0xff).unwrap()
        );
    }

    #[test]
    fn assembles_option_table_with_big_endian_offsets() {
        let bytes = assemble_options(&[
            PreLoginOption {
                token: PreLoginOptionToken::Version,
                data: vec![0, 0, 0, 0, 0, 0],
            },
            PreLoginOption {
                token: PreLoginOptionToken::Encryption,
                data: vec![encode_encrypt(Encrypt::On)],
            },
        ])
        .unwrap();

        assert_eq!(
            vec![
                0x00, 0x00, 0x0b, 0x00, 0x06, // VERSION at offset 11, len 6
                0x01, 0x00, 0x11, 0x00, 0x01, // ENCRYPTION at offset 17, len 1
                0xff, // terminator
                0, 0, 0, 0, 0, 0,    // version payload
                0x01, // encryption payload
            ],
            bytes
        );
    }

    #[test]
    fn parses_option_table_payloads() {
        let options = parse_options(&[
            0x00, 0x00, 0x0b, 0x00, 0x06, 0x01, 0x00, 0x11, 0x00, 0x01, 0xff, 0, 0, 0, 0, 0, 0,
            0x03,
        ])
        .unwrap();

        assert_eq!(
            vec![
                PreLoginOption {
                    token: PreLoginOptionToken::Version,
                    data: vec![0, 0, 0, 0, 0, 0],
                },
                PreLoginOption {
                    token: PreLoginOptionToken::Encryption,
                    data: vec![0x03],
                },
            ],
            options
        );
    }

    #[test]
    fn builds_pre_login_payload_from_connection_options() {
        let options = MssqlConnectOptions::parse_url(
            "mssql://localhost/master?encrypt=not_supported&instance=SQLEXPRESS",
        )
        .unwrap();
        let payload = build_pre_login_payload(&options).unwrap();
        let parsed = parse_options(&payload).unwrap();

        assert!(parsed.iter().any(|option| {
            option.token == PreLoginOptionToken::Encryption && option.data == vec![0x02]
        }));
        assert!(parsed.iter().any(|option| {
            option.token == PreLoginOptionToken::Instance && option.data == b"SQLEXPRESS\0"
        }));
    }

    #[test]
    fn extracts_server_encrypt_option() {
        let payload = assemble_options(&[PreLoginOption {
            token: PreLoginOptionToken::Encryption,
            data: vec![0x02],
        }])
        .unwrap();

        assert_eq!(
            Encrypt::NotSupported,
            parse_server_encrypt(&payload).unwrap()
        );
    }

    #[test]
    fn rejects_pre_login_option_out_of_bounds() {
        let err = parse_options(&[0x01, 0x00, 0x10, 0x00, 0x01, 0xff]).unwrap_err();

        assert_eq!(PreLoginError::OptionOutOfBounds { offset: 16, len: 1 }, err);
    }
}
