use thiserror::Error;

/// TDS tabular-result token type byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenType(u8);

impl TokenType {
    /// ERROR token.
    pub const ERROR: Self = Self(0xaa);
    /// INFO token.
    pub const INFO: Self = Self(0xab);
    /// LOGINACK token.
    pub const LOGINACK: Self = Self(0xad);
    /// ENVCHANGE token.
    pub const ENVCHANGE: Self = Self(0xe3);
    /// DONE token.
    pub const DONE: Self = Self(0xfd);

    /// Returns the raw token type byte.
    pub const fn code(self) -> u8 {
        self.0
    }
}

impl From<u8> for TokenType {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

/// Parsed subset of TDS tabular-result tokens needed during login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Server accepted the LOGIN7 request.
    LoginAck(LoginAck),
    /// Server returned an error.
    Error(ServerError),
    /// Server reported a connection environment change.
    EnvChange(EnvChange),
    /// Server completed the response stream.
    Done(Done),
}

/// LOGINACK token data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginAck {
    /// Accepted server interface.
    pub interface: u8,
    /// TDS protocol version selected by the server.
    pub tds_version: u32,
    /// Server program name.
    pub program_name: String,
    /// Server major version.
    pub major_version: u8,
    /// Server minor version.
    pub minor_version: u8,
    /// High byte of the server build number.
    pub build_number_high: u8,
    /// Low byte of the server build number.
    pub build_number_low: u8,
}

/// ERROR token data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    /// SQL Server error number.
    pub number: i32,
    /// Error state.
    pub state: u8,
    /// Error class / severity.
    pub class: u8,
    /// Human-readable error message.
    pub message: String,
    /// Server name.
    pub server_name: String,
    /// Stored procedure name, when present.
    pub procedure_name: String,
    /// Line number reported by the server.
    pub line_number: u32,
}

/// ENVCHANGE token data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvChange {
    /// Current database changed.
    Database(String),
    /// Current language changed.
    Language(String),
    /// Current character set changed.
    CharacterSet(String),
    /// Server accepted a packet size.
    PacketSize(u32),
    /// Unicode sorting locale changed.
    UnicodeDataSortingLocalId(String),
    /// Unicode sorting comparison flags changed.
    UnicodeDataSortingComparisonFlags(String),
    /// SQL collation changed.
    SqlCollation(Vec<u8>),
    /// Server started a transaction and returned its descriptor.
    BeginTransaction(u64),
    /// Server committed a transaction.
    CommitTransaction(u64),
    /// Server rolled back a transaction.
    RollbackTransaction(u64),
    /// ENVCHANGE type not currently interpreted by this driver.
    Ignored {
        /// Environment change type byte.
        change_type: u8,
        /// Raw change payload after the type byte.
        data: Vec<u8>,
    },
}

/// DONE token data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Done {
    /// DONE status bit field.
    pub status: u16,
    /// Current command token.
    pub current_command: u16,
    /// Rows affected, valid when the DONE_COUNT status bit is set.
    pub row_count: u64,
}

/// Result of interpreting a LOGIN7 response token stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginResponse {
    /// LOGINACK was received and no ERROR token was present.
    Success {
        /// Accepted login metadata returned by the server.
        login_ack: LoginAck,
        /// Environment changes received during login.
        env_changes: Vec<EnvChange>,
    },
    /// Server returned at least one ERROR token.
    ServerError(ServerError),
}

/// Parses the bounded token subset currently needed from a tabular-result payload.
pub fn parse_tokens(mut input: &[u8]) -> Result<Vec<Token>, TokenParseError> {
    let mut tokens = Vec::new();

    while !input.is_empty() {
        let token_type = TokenType::from(read_u8(&mut input)?);

        let token = if token_type == TokenType::LOGINACK {
            Token::LoginAck(parse_login_ack(read_len_prefixed_token(&mut input)?)?)
        } else if token_type == TokenType::ERROR {
            Token::Error(parse_error(read_len_prefixed_token(&mut input)?)?)
        } else if token_type == TokenType::INFO {
            let _ = read_len_prefixed_token(&mut input)?;
            continue;
        } else if token_type == TokenType::ENVCHANGE {
            Token::EnvChange(parse_env_change(read_len_prefixed_token(&mut input)?)?)
        } else if token_type == TokenType::DONE {
            Token::Done(parse_done(&mut input)?)
        } else {
            return Err(TokenParseError::UnsupportedToken(token_type.code()));
        };

        tokens.push(token);
    }

    Ok(tokens)
}

/// Interprets a LOGIN7 response token stream as success or server failure.
pub fn parse_login_response(input: &[u8]) -> Result<LoginResponse, TokenParseError> {
    let tokens = parse_tokens(input)?;
    let mut login_ack = None;
    let mut done = false;
    let mut env_changes = Vec::new();

    for token in tokens {
        match token {
            Token::LoginAck(ack) => login_ack = Some(ack),
            Token::Error(error) => return Ok(LoginResponse::ServerError(error)),
            Token::Done(_) => done = true,
            Token::EnvChange(change) => env_changes.push(change),
        }
    }

    let login_ack = login_ack.ok_or(TokenParseError::MissingLoginAck)?;
    if !done {
        return Err(TokenParseError::MissingDone);
    }

    Ok(LoginResponse::Success {
        login_ack,
        env_changes,
    })
}

fn parse_login_ack(mut input: &[u8]) -> Result<LoginAck, TokenParseError> {
    let interface = read_u8(&mut input)?;
    let tds_version = read_u32_be(&mut input)?;
    let program_name = read_b_varchar(&mut input)?;
    let major_version = read_u8(&mut input)?;
    let minor_version = read_u8(&mut input)?;
    let build_number_high = read_u8(&mut input)?;
    let build_number_low = read_u8(&mut input)?;
    expect_empty(input)?;

    Ok(LoginAck {
        interface,
        tds_version,
        program_name,
        major_version,
        minor_version,
        build_number_high,
        build_number_low,
    })
}

pub(crate) fn parse_server_error(input: &[u8]) -> Result<ServerError, TokenParseError> {
    parse_error(input)
}

fn parse_error(mut input: &[u8]) -> Result<ServerError, TokenParseError> {
    let number = read_i32_le(&mut input)?;
    let state = read_u8(&mut input)?;
    let class = read_u8(&mut input)?;
    let message = read_us_varchar(&mut input)?;
    let server_name = read_b_varchar(&mut input)?;
    let procedure_name = read_b_varchar(&mut input)?;
    let line_number = read_u32_le(&mut input)?;
    expect_empty(input)?;

    Ok(ServerError {
        number,
        state,
        class,
        message,
        server_name,
        procedure_name,
        line_number,
    })
}

pub(crate) fn parse_env_change(mut input: &[u8]) -> Result<EnvChange, TokenParseError> {
    let change_type = read_u8(&mut input)?;

    Ok(match change_type {
        1 => EnvChange::Database(read_b_varchar(&mut input)?),
        2 => EnvChange::Language(read_b_varchar(&mut input)?),
        3 => EnvChange::CharacterSet(read_b_varchar(&mut input)?),
        4 => {
            let size = read_b_varchar(&mut input)?;
            EnvChange::PacketSize(
                size.parse()
                    .map_err(|_| TokenParseError::InvalidEnvChangePacketSize(size))?,
            )
        }
        5 => EnvChange::UnicodeDataSortingLocalId(read_b_varchar(&mut input)?),
        6 => EnvChange::UnicodeDataSortingComparisonFlags(read_b_varchar(&mut input)?),
        7 => EnvChange::SqlCollation(read_b_varbyte(&mut input)?.to_vec()),
        8 => EnvChange::BeginTransaction(read_b_varbyte_u64_le(&mut input)?),
        9 => EnvChange::CommitTransaction(read_transaction_end_descriptor(&mut input)?),
        10 => EnvChange::RollbackTransaction(read_transaction_end_descriptor(&mut input)?),
        _ => EnvChange::Ignored {
            change_type,
            data: input.to_vec(),
        },
    })
}

fn parse_done(input: &mut &[u8]) -> Result<Done, TokenParseError> {
    Ok(Done {
        status: read_u16_le(input)?,
        current_command: read_u16_le(input)?,
        row_count: read_u64_le(input)?,
    })
}

fn read_len_prefixed_token<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], TokenParseError> {
    let len = usize::from(read_u16_le(input)?);
    take(input, len)
}

fn read_b_varchar(input: &mut &[u8]) -> Result<String, TokenParseError> {
    let len_chars = usize::from(read_u8(input)?);
    read_utf16_string(input, len_chars)
}

fn read_b_varbyte<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], TokenParseError> {
    let len = usize::from(read_u8(input)?);
    take(input, len)
}

fn read_us_varchar(input: &mut &[u8]) -> Result<String, TokenParseError> {
    let len_chars = usize::from(read_u16_le(input)?);
    read_utf16_string(input, len_chars)
}

fn read_utf16_string(input: &mut &[u8], len_chars: usize) -> Result<String, TokenParseError> {
    let len_bytes = len_chars
        .checked_mul(2)
        .ok_or(TokenParseError::LengthOverflow)?;
    let bytes = take(input, len_bytes)?;
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]));

    String::from_utf16(&units.collect::<Vec<_>>()).map_err(|_| TokenParseError::InvalidUtf16)
}

fn read_u8(input: &mut &[u8]) -> Result<u8, TokenParseError> {
    let bytes = take(input, 1)?;
    Ok(bytes[0])
}

fn read_u16_le(input: &mut &[u8]) -> Result<u16, TokenParseError> {
    let bytes = take(input, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_i32_le(input: &mut &[u8]) -> Result<i32, TokenParseError> {
    let bytes = take(input, 4)?;
    Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u32_le(input: &mut &[u8]) -> Result<u32, TokenParseError> {
    let bytes = take(input, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u32_be(input: &mut &[u8]) -> Result<u32, TokenParseError> {
    let bytes = take(input, 4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(input: &mut &[u8]) -> Result<u64, TokenParseError> {
    let bytes = take(input, 8)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_b_varbyte_u64_le(input: &mut &[u8]) -> Result<u64, TokenParseError> {
    let mut bytes = read_b_varbyte(input)?;
    read_u64_le(&mut bytes)
}

fn read_transaction_end_descriptor(input: &mut &[u8]) -> Result<u64, TokenParseError> {
    let _new_descriptor = read_b_varbyte(input)?;
    read_u64_le(input)
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], TokenParseError> {
    let bytes = input.get(..len).ok_or(TokenParseError::UnexpectedEof)?;
    *input = &input[len..];
    Ok(bytes)
}

fn expect_empty(input: &[u8]) -> Result<(), TokenParseError> {
    if input.is_empty() {
        Ok(())
    } else {
        Err(TokenParseError::TrailingTokenBytes(input.len()))
    }
}

/// Error returned while parsing a bounded TDS token stream.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TokenParseError {
    /// The token stream ended in the middle of a token.
    #[error("TDS token stream ended before the current token was complete")]
    UnexpectedEof,
    /// A token advertised a length that cannot be represented safely.
    #[error("TDS token length overflowed")]
    LengthOverflow,
    /// A token contained invalid UTF-16 string data.
    #[error("TDS token contained invalid UTF-16 string data")]
    InvalidUtf16,
    /// This bounded parser does not yet understand the token type.
    #[error("unsupported TDS token 0x{0:02x}")]
    UnsupportedToken(u8),
    /// A length-prefixed token contained extra bytes after its expected fields.
    #[error("TDS token contained {0} trailing bytes")]
    TrailingTokenBytes(usize),
    /// A LOGIN7 response did not include LOGINACK.
    #[error("TDS login response did not include LOGINACK")]
    MissingLoginAck,
    /// A LOGIN7 response did not include DONE.
    #[error("TDS login response did not include DONE")]
    MissingDone,
    /// ENVCHANGE packet size was not a decimal integer.
    #[error("TDS ENVCHANGE packet size `{0}` is not a valid integer")]
    InvalidEnvChangePacketSize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_login_ack_envchange_and_done_as_success() {
        let bytes = [
            login_ack("Microsoft SQL Server"),
            env_change(
                4,
                &[
                    4, b'4', 0, b'0', 0, b'9', 0, b'6', 0, 3, b'5', 0, b'1', 0, b'2', 0,
                ],
            ),
            done(0, 0, 0),
        ]
        .concat();

        let tokens = parse_tokens(&bytes).unwrap();

        assert_eq!(3, tokens.len());
        assert_eq!(
            LoginResponse::Success {
                login_ack: LoginAck {
                    interface: 1,
                    tds_version: 0x7400_0004,
                    program_name: "Microsoft SQL Server".to_owned(),
                    major_version: 16,
                    minor_version: 0,
                    build_number_high: 0x10,
                    build_number_low: 0x4a,
                },
                env_changes: vec![EnvChange::PacketSize(4096)],
            },
            parse_login_response(&bytes).unwrap()
        );
    }

    #[test]
    fn parses_transaction_envchanges() {
        assert_eq!(
            EnvChange::BeginTransaction(0x0102_0304_0506_0708),
            parse_env_change(&[8, 8, 8, 7, 6, 5, 4, 3, 2, 1,]).unwrap()
        );
        assert_eq!(
            EnvChange::CommitTransaction(0x1112_1314_1516_1718),
            parse_env_change(&[9, 0, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11,]).unwrap()
        );
        assert_eq!(
            EnvChange::RollbackTransaction(0x2122_2324_2526_2728),
            parse_env_change(&[10, 0, 0x28, 0x27, 0x26, 0x25, 0x24, 0x23, 0x22, 0x21,]).unwrap()
        );
    }

    #[test]
    fn reports_server_error_before_done() {
        let bytes = [
            error(18456, 1, 14, "Login failed", "dbhost", "", 1),
            done(0x0002, 0, 0),
        ]
        .concat();

        assert_eq!(
            LoginResponse::ServerError(ServerError {
                number: 18456,
                state: 1,
                class: 14,
                message: "Login failed".to_owned(),
                server_name: "dbhost".to_owned(),
                procedure_name: String::new(),
                line_number: 1,
            }),
            parse_login_response(&bytes).unwrap()
        );
    }

    #[test]
    fn skips_info_tokens_during_login() {
        let bytes = [
            len_prefixed(
                TokenType::INFO,
                error_body(5701, 1, 10, "Changed database", "", "", 1),
            ),
            login_ack("Microsoft SQL Server"),
            done(0, 0, 0),
        ]
        .concat();

        assert!(matches!(
            parse_login_response(&bytes).unwrap(),
            LoginResponse::Success { .. }
        ));
    }

    #[test]
    fn rejects_truncated_login_ack() {
        let bytes = [TokenType::LOGINACK.code(), 10, 0, 1, 0x74];

        assert_eq!(
            TokenParseError::UnexpectedEof,
            parse_tokens(&bytes).unwrap_err()
        );
    }

    #[test]
    fn rejects_unsupported_tokens_in_bounded_parser() {
        let bytes = [0xac, 0, 0];

        assert_eq!(
            TokenParseError::UnsupportedToken(0xac),
            parse_tokens(&bytes).unwrap_err()
        );
    }

    #[test]
    fn login_response_requires_login_ack_when_no_error_is_present() {
        let bytes = done(0, 0, 0);

        assert_eq!(
            TokenParseError::MissingLoginAck,
            parse_login_response(&bytes).unwrap_err()
        );
    }

    #[test]
    fn login_response_success_requires_done() {
        let bytes = login_ack("Microsoft SQL Server");

        assert_eq!(
            TokenParseError::MissingDone,
            parse_login_response(&bytes).unwrap_err()
        );
    }

    fn login_ack(program_name: &str) -> Vec<u8> {
        let mut body = Vec::new();
        body.push(1);
        body.extend_from_slice(&0x7400_0004u32.to_be_bytes());
        push_b_varchar(&mut body, program_name);
        body.extend_from_slice(&[16, 0, 0x10, 0x4a]);

        len_prefixed(TokenType::LOGINACK, body)
    }

    fn error(
        number: i32,
        state: u8,
        class: u8,
        message: &str,
        server_name: &str,
        procedure_name: &str,
        line_number: u32,
    ) -> Vec<u8> {
        len_prefixed(
            TokenType::ERROR,
            error_body(
                number,
                state,
                class,
                message,
                server_name,
                procedure_name,
                line_number,
            ),
        )
    }

    fn error_body(
        number: i32,
        state: u8,
        class: u8,
        message: &str,
        server_name: &str,
        procedure_name: &str,
        line_number: u32,
    ) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&number.to_le_bytes());
        body.push(state);
        body.push(class);
        push_us_varchar(&mut body, message);
        push_b_varchar(&mut body, server_name);
        push_b_varchar(&mut body, procedure_name);
        body.extend_from_slice(&line_number.to_le_bytes());
        body
    }

    fn env_change(change_type: u8, data: &[u8]) -> Vec<u8> {
        let mut body = Vec::with_capacity(1 + data.len());
        body.push(change_type);
        body.extend_from_slice(data);

        len_prefixed(TokenType::ENVCHANGE, body)
    }

    fn done(status: u16, current_command: u16, row_count: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TokenType::DONE.code());
        out.extend_from_slice(&status.to_le_bytes());
        out.extend_from_slice(&current_command.to_le_bytes());
        out.extend_from_slice(&row_count.to_le_bytes());
        out
    }

    fn len_prefixed(token_type: TokenType, body: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(token_type.code());
        out.extend_from_slice(
            &u16::try_from(body.len())
                .expect("test token body fits in u16")
                .to_le_bytes(),
        );
        out.extend_from_slice(&body);
        out
    }

    fn push_b_varchar(out: &mut Vec<u8>, value: &str) {
        out.push(u8::try_from(value.encode_utf16().count()).expect("test string fits in u8"));
        push_utf16(out, value);
    }

    fn push_us_varchar(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(
            &u16::try_from(value.encode_utf16().count())
                .expect("test string fits in u16")
                .to_le_bytes(),
        );
        push_utf16(out, value);
    }

    fn push_utf16(out: &mut Vec<u8>, value: &str) {
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
}
