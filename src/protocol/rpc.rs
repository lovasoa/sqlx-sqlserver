use sqlx_core::error::BoxDynError;

use super::packet::{encode_message, PacketFrameError, PacketType};
use super::query::write_all_headers;
use crate::arguments::write_nvarchar_parameter;
use crate::MssqlArguments;

const PROCEDURE_ID_EXECUTE_SQL: u16 = 10;

pub(crate) fn build_execute_sql_packet(
    sql: &str,
    arguments: &MssqlArguments,
    packet_size: usize,
    transaction_descriptor: u64,
) -> Result<Vec<u8>, RpcPacketError> {
    let mut payload = Vec::with_capacity(22 + sql.len() * 2 + arguments.data().len() + 32);
    write_all_headers(&mut payload, transaction_descriptor);

    payload.extend_from_slice(&u16::MAX.to_le_bytes());
    payload.extend_from_slice(&PROCEDURE_ID_EXECUTE_SQL.to_le_bytes());
    payload.extend_from_slice(&0_u16.to_le_bytes());

    write_nvarchar_parameter(&mut payload, "", sql).map_err(RpcPacketError::Argument)?;

    if !arguments.is_empty() {
        write_nvarchar_parameter(&mut payload, "", arguments.declarations())
            .map_err(RpcPacketError::Argument)?;
        payload.extend_from_slice(arguments.data());
    }

    encode_message(PacketType::RPC, &payload, packet_size).map_err(RpcPacketError::Frame)
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

    fn utf16_bytes(value: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for unit in value.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    }
}
