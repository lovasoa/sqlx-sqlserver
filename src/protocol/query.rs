use sqlx_core::column::Column;
use sqlx_core::Error;

use super::packet::{encode_message, PacketFrameError, PacketType};
use super::token::{parse_server_error, ServerError, TokenParseError};
use crate::{MssqlColumn, MssqlQueryResult, MssqlRow, MssqlType, MssqlTypeInfo, MssqlValue};

const TOKEN_COL_METADATA: u8 = 0x81;
const TOKEN_ERROR: u8 = 0xaa;
const TOKEN_INFO: u8 = 0xab;
const TOKEN_ROW: u8 = 0xd1;
const TOKEN_ENVCHANGE: u8 = 0xe3;
const TOKEN_DONE: u8 = 0xfd;
const TOKEN_DONEPROC: u8 = 0xfe;
const TOKEN_DONEINPROC: u8 = 0xff;

const DATA_TYPE_TINYINT: u8 = 0x30;
const DATA_TYPE_SMALLINT: u8 = 0x34;
const DATA_TYPE_INT: u8 = 0x38;
const DATA_TYPE_BIGINT: u8 = 0x7f;
const DATA_TYPE_INTN: u8 = 0x26;

const DONE_COUNT: u16 = 0x0010;

#[derive(Debug)]
pub(crate) struct QueryOutput {
    pub(crate) rows: Vec<MssqlRow>,
    pub(crate) result: MssqlQueryResult,
}

pub(crate) fn build_sql_batch_packet(
    sql: &str,
    packet_size: usize,
    transaction_descriptor: u64,
) -> Result<Vec<u8>, PacketFrameError> {
    let mut payload = Vec::with_capacity(22 + sql.len() * 2);
    write_all_headers(&mut payload, transaction_descriptor);

    for unit in sql.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }

    encode_message(PacketType::SQL_BATCH, &payload, packet_size)
}

pub(crate) fn parse_query_response(input: &[u8]) -> Result<QueryOutput, Error> {
    let mut input = input;
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    let mut rows_affected = 0;

    while !input.is_empty() {
        let token = read_u8(&mut input)?;

        match token {
            TOKEN_COL_METADATA => columns = parse_col_metadata(&mut input)?,
            TOKEN_ROW => rows.push(parse_row(&mut input, &columns)?),
            TOKEN_DONE | TOKEN_DONEPROC | TOKEN_DONEINPROC => {
                let done = parse_done(&mut input)?;
                if done.status & DONE_COUNT != 0 {
                    rows_affected += done.row_count;
                }
            }
            TOKEN_ERROR => {
                let error = parse_server_error(read_len_prefixed(&mut input)?)
                    .map_err(token_parse_error)?;
                return Err(server_error(error));
            }
            TOKEN_INFO | TOKEN_ENVCHANGE => {
                let _ = read_len_prefixed(&mut input)?;
            }
            other => {
                return Err(Error::Protocol(format!(
                    "unsupported SQL Server query token 0x{other:02x}"
                )));
            }
        }
    }

    Ok(QueryOutput {
        rows,
        result: MssqlQueryResult::new(rows_affected),
    })
}

fn write_all_headers(out: &mut Vec<u8>, transaction_descriptor: u64) {
    out.extend_from_slice(&22_u32.to_le_bytes());
    out.extend_from_slice(&18_u32.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&transaction_descriptor.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
}

fn parse_col_metadata(input: &mut &[u8]) -> Result<Vec<MssqlColumn>, Error> {
    let count = read_u16_le(input)?;
    if count == 0xffff {
        return Ok(Vec::new());
    }

    let mut columns = Vec::with_capacity(usize::from(count));
    for ordinal in 0..usize::from(count) {
        let _user_type = read_u32_le(input)?;
        let _flags = read_u16_le(input)?;
        let type_info = parse_type_info(input)?;
        let name = read_b_varchar(input)?;

        columns.push(MssqlColumn::new(ordinal, name, type_info));
    }

    Ok(columns)
}

fn parse_type_info(input: &mut &[u8]) -> Result<MssqlTypeInfo, Error> {
    let ty = read_u8(input)?;

    Ok(match ty {
        DATA_TYPE_TINYINT => MssqlTypeInfo::TINYINT,
        DATA_TYPE_SMALLINT => MssqlTypeInfo::SMALLINT,
        DATA_TYPE_INT => MssqlTypeInfo::INT,
        DATA_TYPE_BIGINT => MssqlTypeInfo::BIGINT,
        DATA_TYPE_INTN => match read_u8(input)? {
            1 => MssqlTypeInfo::TINYINT,
            2 => MssqlTypeInfo::SMALLINT,
            4 => MssqlTypeInfo::INT,
            8 => MssqlTypeInfo::BIGINT,
            size => {
                return Err(Error::Protocol(format!(
                    "unsupported SQL Server INTN size {size}"
                )));
            }
        },
        other => MssqlTypeInfo::new(MssqlType::Other(format!("TDS_TYPE_0x{other:02x}"))),
    })
}

fn parse_row(input: &mut &[u8], columns: &[MssqlColumn]) -> Result<MssqlRow, Error> {
    let mut values = Vec::with_capacity(columns.len());

    for column in columns {
        let value = match column.type_info().kind() {
            MssqlType::TinyInt => {
                MssqlValue::new(column.type_info().clone(), Some(take(input, 1)?.to_vec()))
            }
            MssqlType::SmallInt => {
                MssqlValue::new(column.type_info().clone(), Some(take(input, 2)?.to_vec()))
            }
            MssqlType::Int => {
                MssqlValue::new(column.type_info().clone(), Some(take(input, 4)?.to_vec()))
            }
            MssqlType::BigInt => {
                MssqlValue::new(column.type_info().clone(), Some(take(input, 8)?.to_vec()))
            }
            other => {
                return Err(Error::Protocol(format!(
                    "SQL Server row decoding does not support type {other:?}"
                )));
            }
        };

        values.push(value);
    }

    Ok(MssqlRow::new(columns.to_vec(), values))
}

fn parse_done(input: &mut &[u8]) -> Result<Done, Error> {
    Ok(Done {
        status: read_u16_le(input)?,
        _current_command: read_u16_le(input)?,
        row_count: read_u64_le(input)?,
    })
}

struct Done {
    status: u16,
    _current_command: u16,
    row_count: u64,
}

fn read_len_prefixed<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], Error> {
    let len = usize::from(read_u16_le(input)?);
    take(input, len)
}

fn read_b_varchar(input: &mut &[u8]) -> Result<String, Error> {
    let len_chars = usize::from(read_u8(input)?);
    read_utf16(input, len_chars)
}

fn read_utf16(input: &mut &[u8], len_chars: usize) -> Result<String, Error> {
    let len_bytes = len_chars
        .checked_mul(2)
        .ok_or_else(|| Error::Protocol("SQL Server string length overflow".to_owned()))?;
    let bytes = take(input, len_bytes)?;
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();

    String::from_utf16(&units)
        .map_err(|_| Error::Protocol("SQL Server string contained invalid UTF-16".to_owned()))
}

fn read_u8(input: &mut &[u8]) -> Result<u8, Error> {
    Ok(take(input, 1)?[0])
}

fn read_u16_le(input: &mut &[u8]) -> Result<u16, Error> {
    let bytes = take(input, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(input: &mut &[u8]) -> Result<u32, Error> {
    let bytes = take(input, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(input: &mut &[u8]) -> Result<u64, Error> {
    let bytes = take(input, 8)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], Error> {
    let bytes = input.get(..len).ok_or_else(|| {
        Error::Protocol("SQL Server query token ended before expected length".to_owned())
    })?;
    *input = &input[len..];
    Ok(bytes)
}

fn server_error(error: ServerError) -> Error {
    Error::Protocol(format!(
        "SQL Server error {} (state {}, class {}): {}",
        error.number, error.state, error.class, error.message
    ))
}

fn token_parse_error(error: TokenParseError) -> Error {
    Error::Protocol(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx_core::row::Row;

    #[test]
    fn sql_batch_packet_starts_with_all_headers_and_utf16_sql() {
        let packet = build_sql_batch_packet("SELECT 1", 512, 0).unwrap();
        let payload = &packet[8..];

        assert_eq!(22, u32::from_le_bytes(payload[0..4].try_into().unwrap()));
        assert_eq!(18, u32::from_le_bytes(payload[4..8].try_into().unwrap()));
        assert_eq!(2, u16::from_le_bytes(payload[8..10].try_into().unwrap()));
        assert_eq!(0, u64::from_le_bytes(payload[10..18].try_into().unwrap()));
        assert_eq!(1, u32::from_le_bytes(payload[18..22].try_into().unwrap()));
        assert_eq!(
            &[b'S', 0, b'E', 0, b'L', 0, b'E', 0, b'C', 0, b'T', 0, b' ', 0, b'1', 0],
            &payload[22..]
        );
    }

    #[test]
    fn parses_select_one_response() {
        let response = [col_metadata_int(""), row_int(1), done(0x10, 0, 1)].concat();
        let output = parse_query_response(&response).unwrap();

        assert_eq!(1, output.rows.len());
        assert_eq!(1, output.result.rows_affected());
        assert_eq!(1_i32, output.rows[0].try_get::<i32, _>(0).unwrap());
    }

    fn col_metadata_int(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(DATA_TYPE_INT);
        push_b_varchar(&mut out, name);
        out
    }

    fn row_int(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_ROW);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn done(status: u16, current_command: u16, row_count: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_DONE);
        out.extend_from_slice(&status.to_le_bytes());
        out.extend_from_slice(&current_command.to_le_bytes());
        out.extend_from_slice(&row_count.to_le_bytes());
        out
    }

    fn push_b_varchar(out: &mut Vec<u8>, value: &str) {
        out.push(u8::try_from(value.encode_utf16().count()).unwrap());
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
}
