use sqlx_core::error::BoxDynError;

use super::packet::{encode_message, PacketFrameError, PacketType};
use super::query::write_all_headers;
use crate::arguments::{
    type_declaration, write_null_nvarchar_parameter, write_nvarchar_parameter,
    write_output_i32_parameter, write_parameter,
};
use crate::{MssqlArguments, MssqlTypeInfo};

const PROCEDURE_ID_EXECUTE_SQL: u16 = 10;
const PROCEDURE_ID_PREPARE: u16 = 11;
const PROCEDURE_ID_UNPREPARE: u16 = 15;

pub(crate) fn build_execute_sql_packet(
    sql: &str,
    arguments: &MssqlArguments,
    packet_size: usize,
    transaction_descriptor: u64,
) -> Result<Vec<u8>, RpcPacketError> {
    let mut payload = Vec::with_capacity(22 + sql.len() * 2 + arguments.data().len() + 32);
    write_rpc_header(
        &mut payload,
        PROCEDURE_ID_EXECUTE_SQL,
        transaction_descriptor,
    );

    write_nvarchar_parameter(&mut payload, "", sql).map_err(RpcPacketError::Argument)?;

    if !arguments.is_empty() {
        write_nvarchar_parameter(&mut payload, "", arguments.declarations())
            .map_err(RpcPacketError::Argument)?;
        payload.extend_from_slice(arguments.data());
    }

    encode_message(PacketType::RPC, &payload, packet_size).map_err(RpcPacketError::Frame)
}

pub(crate) fn build_prepare_packet(
    sql: &str,
    parameters: &[MssqlTypeInfo],
    packet_size: usize,
    transaction_descriptor: u64,
) -> Result<Vec<u8>, RpcPacketError> {
    let mut payload = Vec::with_capacity(22 + sql.len() * 2 + parameters.len() * 16 + 64);
    write_rpc_header(&mut payload, PROCEDURE_ID_PREPARE, transaction_descriptor);

    write_output_i32_parameter(&mut payload, "", 0).map_err(RpcPacketError::Argument)?;

    if parameters.is_empty() {
        write_null_nvarchar_parameter(&mut payload, "").map_err(RpcPacketError::Argument)?;
    } else {
        write_nvarchar_parameter(&mut payload, "", &parameter_declarations(parameters)?)
            .map_err(RpcPacketError::Argument)?;
    }

    write_nvarchar_parameter(&mut payload, "", sql).map_err(RpcPacketError::Argument)?;
    write_parameter(
        &mut payload,
        "",
        &MssqlTypeInfo::INT,
        &1_i32.to_le_bytes(),
        false,
    )
    .map_err(RpcPacketError::Argument)?;

    encode_message(PacketType::RPC, &payload, packet_size).map_err(RpcPacketError::Frame)
}

pub(crate) fn build_unprepare_packet(
    statement_id: i32,
    packet_size: usize,
    transaction_descriptor: u64,
) -> Result<Vec<u8>, RpcPacketError> {
    let mut payload = Vec::with_capacity(40);
    write_rpc_header(&mut payload, PROCEDURE_ID_UNPREPARE, transaction_descriptor);
    write_parameter(
        &mut payload,
        "",
        &MssqlTypeInfo::INT,
        &statement_id.to_le_bytes(),
        false,
    )
    .map_err(RpcPacketError::Argument)?;

    encode_message(PacketType::RPC, &payload, packet_size).map_err(RpcPacketError::Frame)
}

fn write_rpc_header(out: &mut Vec<u8>, procedure_id: u16, transaction_descriptor: u64) {
    write_all_headers(out, transaction_descriptor);
    out.extend_from_slice(&u16::MAX.to_le_bytes());
    out.extend_from_slice(&procedure_id.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
}

fn parameter_declarations(parameters: &[MssqlTypeInfo]) -> Result<String, RpcPacketError> {
    let mut declarations = String::new();

    for (idx, type_info) in parameters.iter().enumerate() {
        if !declarations.is_empty() {
            declarations.push(',');
        }

        declarations.push_str("@p");
        declarations.push_str(&(idx + 1).to_string());
        declarations.push(' ');
        declarations.push_str(type_declaration(type_info).map_err(RpcPacketError::Argument)?);
    }

    Ok(declarations)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RpcPacketError {
    #[error("failed to encode SQL Server RPC argument: {0}")]
    Argument(BoxDynError),
    #[error(transparent)]
    Frame(#[from] PacketFrameError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx_core::arguments::Arguments;

    #[test]
    fn rpc_packet_uses_execute_sql_builtin_proc_and_arguments() {
        let mut args = MssqlArguments::default();
        args.add(7_i32).unwrap();

        let packet = build_execute_sql_packet("SELECT @p1", &args, 512, 0).unwrap();
        let payload = &packet[8..];

        assert_eq!(22, u32::from_le_bytes(payload[0..4].try_into().unwrap()));
        assert_eq!([0xff, 0xff, 10, 0, 0, 0], payload[22..28]);
        assert!(payload
            .windows(b"@p1 int".len() * 2)
            .any(|bytes| bytes == utf16_bytes("@p1 int")));
        assert!(payload.windows(5).any(|bytes| bytes == [0x26, 4, 4, 7, 0]));
    }

    #[test]
    fn prepare_packet_uses_prepare_builtin_proc_and_send_metadata() {
        let packet = build_prepare_packet("SELECT 1", &[], 512, 0).unwrap();
        let payload = &packet[8..];

        assert_eq!([0xff, 0xff, 11, 0, 0, 0], payload[22..28]);
        assert!(payload.windows(5).any(|bytes| bytes == [0x26, 4, 4, 1, 0]));
    }

    fn utf16_bytes(value: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    }
}
