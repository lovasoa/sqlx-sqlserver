use sqlx_core::Error;
use std::sync::Arc;

use super::col_meta_data::ColMetaData;
use super::done::{Done, Status};
use super::packet::{encode_message, PacketFrameError, PacketType};
use super::read::{read_len_prefixed, read_u32_le, read_u8};
use super::return_value::ReturnValue;
use super::row::Row;
use super::token::{parse_env_change, parse_server_error, EnvChange, TokenParseError};
use crate::{error::server_error, MssqlColumn, MssqlQueryResult, MssqlRow, MssqlValue};

const TOKEN_COL_METADATA: u8 = 0x81;
const TOKEN_ERROR: u8 = 0xaa;
const TOKEN_INFO: u8 = 0xab;
const TOKEN_RETURN_STATUS: u8 = 0x79;
const TOKEN_RETURN_VALUE: u8 = 0xac;
const TOKEN_ROW: u8 = 0xd1;
const TOKEN_NBCROW: u8 = 0xd2;
const TOKEN_ENVCHANGE: u8 = 0xe3;
const TOKEN_DONE: u8 = 0xfd;
const TOKEN_DONEPROC: u8 = 0xfe;
const TOKEN_DONEINPROC: u8 = 0xff;

#[derive(Debug)]
pub(crate) struct QueryOutput {
    pub(crate) columns: Vec<MssqlColumn>,
    pub(crate) rows: Vec<MssqlRow>,
    pub(crate) result: MssqlQueryResult,
    pub(crate) return_values: Vec<MssqlValue>,
    pub(crate) env_changes: Vec<EnvChange>,
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
    let mut columns = Arc::<[MssqlColumn]>::from(Vec::new());
    let mut rows = Vec::new();
    let mut return_values = Vec::new();
    let mut env_changes = Vec::new();
    let mut rows_affected = 0;

    while !input.is_empty() {
        let token = read_u8(&mut input)?;

        match token {
            TOKEN_COL_METADATA => columns = ColMetaData::get(&mut input)?.into(),
            TOKEN_ROW => rows.push(Row::get(&mut input, false, Arc::clone(&columns))?),
            TOKEN_NBCROW => rows.push(Row::get(&mut input, true, Arc::clone(&columns))?),
            TOKEN_RETURN_VALUE => {
                return_values.push(ReturnValue::get(&mut input)?.into_value());
            }
            TOKEN_RETURN_STATUS => {
                let _ = read_u32_le(&mut input)?;
            }
            TOKEN_DONE | TOKEN_DONEPROC | TOKEN_DONEINPROC => {
                let done = Done::get(&mut input)?;
                if done.status.contains(Status::DONE_COUNT) {
                    rows_affected += done.affected_rows;
                }
            }
            TOKEN_ERROR => {
                let error = parse_server_error(read_len_prefixed(&mut input)?)
                    .map_err(token_parse_error)?;
                return Err(server_error(error));
            }
            TOKEN_ENVCHANGE => {
                env_changes.push(
                    parse_env_change(read_len_prefixed(&mut input)?).map_err(token_parse_error)?,
                );
            }
            TOKEN_INFO => {
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
        columns: columns.to_vec(),
        rows,
        result: MssqlQueryResult::new(rows_affected),
        return_values,
        env_changes,
    })
}

pub(crate) fn write_all_headers(out: &mut Vec<u8>, transaction_descriptor: u64) {
    out.extend_from_slice(&22_u32.to_le_bytes());
    out.extend_from_slice(&18_u32.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&transaction_descriptor.to_le_bytes());
    out.extend_from_slice(&1_u32.to_le_bytes());
}

fn token_parse_error(error: TokenParseError) -> Error {
    Error::Protocol(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mssql;
    use sqlx_core::row::Row;
    use sqlx_core::value::{Value, ValueRef};

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
    fn sql_batch_packet_writes_transaction_descriptor() {
        let packet = build_sql_batch_packet("SELECT 1", 512, 0x0102_0304_0506_0708).unwrap();
        let payload = &packet[8..];

        assert_eq!(
            0x0102_0304_0506_0708,
            u64::from_le_bytes(payload[10..18].try_into().unwrap())
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

    #[test]
    fn parses_variable_length_int_response() {
        let response = [col_metadata_intn(""), row_intn(7), done(0x10, 0, 1)].concat();
        let output = parse_query_response(&response).unwrap();

        assert_eq!(7_i32, output.rows[0].try_get::<i32, _>(0).unwrap());
    }

    #[test]
    fn parses_null_typed_value_as_null() {
        let response = [col_metadata_null("value"), row_null(), done(0x10, 0, 1)].concat();
        let output = parse_query_response(&response).unwrap();

        assert!(output.rows[0].try_get_raw(0).unwrap().is_null());
    }

    #[test]
    fn parses_nbcrow_null_bitmap() {
        let response = [col_metadata_intn("value"), nbcrow_null(1), done(0x10, 0, 1)].concat();
        let output = parse_query_response(&response).unwrap();

        assert!(output.rows[0].try_get_raw(0).unwrap().is_null());
    }

    #[test]
    fn parses_return_value_response() {
        let response = [return_status(0), return_value_int(42), done(0x10, 0, 1)].concat();
        let output = parse_query_response(&response).unwrap();

        assert_eq!(1, output.return_values.len());
        assert_eq!(
            42_i32,
            <i32 as sqlx_core::decode::Decode<Mssql>>::decode(output.return_values[0].as_ref())
                .unwrap()
        );
    }

    #[test]
    fn collects_envchange_tokens_from_query_response() {
        let response = [
            env_change(4, &[4, b'8', 0, b'1', 0, b'9', 0, b'2', 0]),
            env_change(8, &[8, 8, 7, 6, 5, 4, 3, 2, 1]),
            done(0, 0, 0),
        ]
        .concat();
        let output = parse_query_response(&response).unwrap();

        assert_eq!(
            output.env_changes,
            vec![
                EnvChange::PacketSize(8192),
                EnvChange::BeginTransaction(0x0102_0304_0506_0708)
            ]
        );
    }

    #[test]
    fn parses_error_token_as_database_error() {
        let response = [error(208, 1, 16, "Invalid object name", "dbhost", "", 3)].concat();
        let error = parse_query_response(&response).unwrap_err();
        let db_error = error.as_database_error().unwrap();
        let mssql_error = db_error
            .as_error()
            .downcast_ref::<crate::MssqlDatabaseError>()
            .unwrap();

        assert_eq!(208, mssql_error.number());
        assert_eq!("Invalid object name", mssql_error.message());
        assert_eq!("dbhost", mssql_error.server_name());
        assert_eq!(3, mssql_error.line_number());
    }

    fn col_metadata_int(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::Int as u8);
        push_b_varchar(&mut out, name);
        out
    }

    fn col_metadata_intn(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::IntN as u8);
        out.push(4);
        push_b_varchar(&mut out, name);
        out
    }

    fn col_metadata_null(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_COL_METADATA);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::Null as u8);
        push_b_varchar(&mut out, name);
        out
    }

    fn row_int(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_ROW);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn row_intn(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_ROW);
        out.push(4);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn row_null() -> Vec<u8> {
        vec![TOKEN_ROW]
    }

    fn nbcrow_null(column_count: usize) -> Vec<u8> {
        let mut out = vec![TOKEN_NBCROW];
        out.resize(1 + column_count.div_ceil(8), 0xff);
        out
    }

    fn return_value_int(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_RETURN_VALUE);
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.push(0);
        out.push(1);
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.push(crate::protocol::type_info::DataType::IntN as u8);
        out.push(4);
        out.push(4);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn return_status(value: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(TOKEN_RETURN_STATUS);
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    fn env_change(change_type: u8, data: &[u8]) -> Vec<u8> {
        let len = 1 + data.len();
        let mut out = Vec::new();
        out.push(TOKEN_ENVCHANGE);
        out.extend_from_slice(&u16::try_from(len).unwrap().to_le_bytes());
        out.push(change_type);
        out.extend_from_slice(data);
        out
    }

    fn error(
        number: i32,
        state: u8,
        class: u8,
        message: &str,
        server: &str,
        procedure: &str,
        line: u32,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&number.to_le_bytes());
        payload.push(state);
        payload.push(class);
        push_us_varchar(&mut payload, message);
        push_b_varchar(&mut payload, server);
        push_b_varchar(&mut payload, procedure);
        payload.extend_from_slice(&line.to_le_bytes());

        let mut out = Vec::new();
        out.push(TOKEN_ERROR);
        out.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&payload);
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

    fn push_us_varchar(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(
            &u16::try_from(value.encode_utf16().count())
                .unwrap()
                .to_le_bytes(),
        );
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn push_b_varchar(out: &mut Vec<u8>, value: &str) {
        out.push(u8::try_from(value.encode_utf16().count()).unwrap());
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
}
