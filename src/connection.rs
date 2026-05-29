use futures_core::future::BoxFuture;
use futures_core::stream::BoxStream;
use futures_util::{future, stream, StreamExt};
use native_tls::Certificate;
use sqlx_core::connection::Connection;
use sqlx_core::decode::Decode;
use sqlx_core::error::Error;
use sqlx_core::executor::{Execute, Executor};
use sqlx_core::transaction::Transaction;
use sqlx_core::value::Value;
use sqlx_core::Either;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_native_tls::TlsConnector;

use crate::error::server_error;
use crate::protocol::login::{build_login7_packet, Login7Error};
use crate::protocol::packet::{PacketHeader, PacketStatus, PacketType, PACKET_HEADER_LEN};
use crate::protocol::pre_login::{build_pre_login_packet, parse_server_encrypt, PreLoginError};
use crate::protocol::query::{build_sql_batch_packet, parse_query_response, QueryOutput};
use crate::protocol::rpc::{
    build_execute_sql_packet, build_prepare_packet, build_unprepare_packet,
};
use crate::protocol::token::{parse_login_response, EnvChange, LoginResponse, TokenParseError};
use crate::tls::TlsPreloginStream;
use crate::{
    ssrp, Encrypt, Mssql, MssqlArguments, MssqlConnectOptions, MssqlQueryResult, MssqlRow,
    MssqlStatement, MssqlTypeInfo,
};

/// SQL Server connection.
#[derive(Debug)]
pub struct MssqlConnection {
    stream: Option<MssqlWireStream>,
    transaction_depth: usize,
    transaction_descriptor: u64,
    pending_rollback_sql: Option<&'static str>,
}

impl MssqlConnection {
    /// Establishes a SQL Server TCP connection and completes PRELOGIN and LOGIN7.
    pub async fn establish(options: &MssqlConnectOptions) -> Result<Self, Error> {
        let mut stream = MssqlWireStream::connect(options).await?;

        let pre_login = build_pre_login_packet(options).map_err(pre_login_error)?;
        stream.write_all(&pre_login).await?;

        let pre_login_response = stream.read_message().await?;
        if pre_login_response.packet_type != PacketType::TABULAR_RESULT {
            return Err(Error::Protocol(format!(
                "expected SQL Server PRELOGIN response as tabular result, got packet type 0x{:02x}",
                pre_login_response.packet_type.code()
            )));
        }

        let server_encrypt =
            parse_server_encrypt(&pre_login_response.payload).map_err(pre_login_error)?;
        let encrypted = negotiate_encryption(options.encrypt(), server_encrypt)?;

        if encrypted {
            stream.enable_tls(options).await?;
        }

        let login = build_login7_packet(options).map_err(login_error)?;
        stream.write_all(&login).await?;

        let login_response = stream.read_message().await?;
        if login_response.packet_type != PacketType::TABULAR_RESULT {
            return Err(Error::Protocol(format!(
                "expected SQL Server LOGIN7 response as tabular result, got packet type 0x{:02x}",
                login_response.packet_type.code()
            )));
        }

        match parse_login_response(&login_response.payload).map_err(token_error)? {
            LoginResponse::Success { env_changes, .. } => {
                let mut conn = Self {
                    stream: Some(stream),
                    transaction_depth: 0,
                    transaction_descriptor: 0,
                    pending_rollback_sql: None,
                };
                conn.apply_env_changes(&env_changes);
                Ok(conn)
            }
            LoginResponse::ServerError(error) => Err(server_error(error)),
        }
    }

    fn apply_env_changes(&mut self, env_changes: &[EnvChange]) {
        for change in env_changes {
            match change {
                EnvChange::PacketSize(size) => {
                    if let Some(stream) = self.stream.as_mut() {
                        stream.packet_size = (*size).clamp(512, 32767) as usize;
                    }
                }
                EnvChange::BeginTransaction(descriptor) => {
                    self.transaction_descriptor = *descriptor;
                }
                EnvChange::CommitTransaction(_) | EnvChange::RollbackTransaction(_) => {
                    self.transaction_descriptor = 0;
                }
                _ => {}
            }
        }
    }

    /// Returns the current transaction depth tracked by the connection.
    pub const fn transaction_depth(&self) -> usize {
        self.transaction_depth
    }

    pub(crate) fn increment_transaction_depth(&mut self) {
        self.transaction_depth += 1;
    }

    pub(crate) fn decrement_transaction_depth(&mut self) {
        self.transaction_depth = self.transaction_depth.saturating_sub(1);
    }

    pub(crate) fn clear_transaction_depth(&mut self) {
        self.transaction_depth = 0;
    }

    pub(crate) async fn run_sql_batch(&mut self, sql: &str) -> Result<QueryOutput, Error> {
        self.flush_pending_rollback().await?;
        self.run_sql_batch_direct(sql).await
    }

    async fn run_sql_batch_direct(&mut self, sql: &str) -> Result<QueryOutput, Error> {
        let transaction_descriptor = self.transaction_descriptor;
        let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
        let packet = build_sql_batch_packet(sql, stream.packet_size, transaction_descriptor)
            .map_err(frame_error)?;
        stream.write_all(&packet).await?;

        self.read_query_response().await
    }

    pub(crate) fn queue_rollback(&mut self) {
        let sql = match self.transaction_depth {
            0 => return,
            1 => {
                self.transaction_depth = 0;
                "ROLLBACK TRANSACTION"
            }
            _ => {
                self.transaction_depth -= 1;
                "ROLLBACK TRANSACTION sqlx_savepoint"
            }
        };

        self.pending_rollback_sql = Some(sql);
    }

    async fn flush_pending_rollback(&mut self) -> Result<(), Error> {
        let Some(sql) = self.pending_rollback_sql.take() else {
            return Ok(());
        };

        self.run_sql_batch_direct(sql).await?;
        Ok(())
    }

    pub(crate) async fn run_execute_sql(
        &mut self,
        sql: &str,
        arguments: Option<&MssqlArguments>,
    ) -> Result<QueryOutput, Error> {
        self.flush_pending_rollback().await?;

        match arguments {
            Some(arguments) if !arguments.is_empty() => {
                let transaction_descriptor = self.transaction_descriptor;
                let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
                let packet = build_execute_sql_packet(
                    sql,
                    arguments,
                    stream.packet_size,
                    transaction_descriptor,
                )
                .map_err(|error| {
                    Error::Protocol(format!("failed to encode SQL Server RPC: {error}"))
                })?;
                stream.write_all(&packet).await?;
                self.read_query_response().await
            }
            _ => self.run_sql_batch_direct(sql).await,
        }
    }

    pub(crate) async fn run_prepare(
        &mut self,
        sql: &str,
        parameters: &[MssqlTypeInfo],
    ) -> Result<QueryOutput, Error> {
        self.flush_pending_rollback().await?;

        let transaction_descriptor = self.transaction_descriptor;
        let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
        let packet =
            build_prepare_packet(sql, parameters, stream.packet_size, transaction_descriptor)
                .map_err(|error| {
                    Error::Protocol(format!("failed to encode SQL Server prepare RPC: {error}"))
                })?;
        stream.write_all(&packet).await?;

        let output = self.read_query_response().await?;

        if let Some(statement_id) = first_i32_return_value(&output)? {
            let transaction_descriptor = self.transaction_descriptor;
            let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
            let packet =
                build_unprepare_packet(statement_id, stream.packet_size, transaction_descriptor)
                    .map_err(|error| {
                        Error::Protocol(format!(
                            "failed to encode SQL Server unprepare RPC: {error}"
                        ))
                    })?;
            stream.write_all(&packet).await?;
            let _ = self.read_query_response().await?;
        }

        Ok(output)
    }

    async fn read_query_response(&mut self) -> Result<QueryOutput, Error> {
        let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
        let response = stream.read_message().await?;
        if response.packet_type != PacketType::TABULAR_RESULT {
            return Err(Error::Protocol(format!(
                "expected SQL Server query response as tabular result, got packet type 0x{:02x}",
                response.packet_type.code()
            )));
        }

        let output = parse_query_response(&response.payload)?;
        self.apply_env_changes(&output.env_changes);
        Ok(output)
    }
}

impl Connection for MssqlConnection {
    type Database = Mssql;
    type Options = MssqlConnectOptions;

    async fn close(mut self) -> Result<(), Error> {
        self.flush_pending_rollback().await?;

        if let Some(mut stream) = self.stream.take() {
            stream.shutdown().await?;
        }

        Ok(())
    }

    async fn close_hard(mut self) -> Result<(), Error> {
        if let Some(mut stream) = self.stream.take() {
            stream.shutdown().await?;
        }

        Ok(())
    }

    async fn ping(&mut self) -> Result<(), Error> {
        self.flush_pending_rollback().await?;

        if self.stream.is_some() {
            Ok(())
        } else {
            Err(wire_not_implemented())
        }
    }

    fn begin(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Transaction<'_, Self::Database>, Error>> + Send + '_
    {
        Transaction::begin(self, None)
    }

    fn shrink_buffers(&mut self) {}

    async fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn should_flush(&self) -> bool {
        false
    }
}

impl<'c> Executor<'c> for &'c mut MssqlConnection {
    type Database = Mssql;

    fn fetch_many<'e, 'q, E>(
        self,
        mut query: E,
    ) -> BoxStream<'e, Result<Either<MssqlQueryResult, MssqlRow>, Error>>
    where
        'c: 'e,
        E: Execute<'q, Self::Database>,
        'q: 'e,
        E: 'q,
    {
        let arguments = query.take_arguments().map_err(Error::Encode);
        let sql = query.sql();

        stream::once(async move {
            let arguments = arguments?;
            self.run_execute_sql(sql.as_str(), arguments.as_ref()).await
        })
        .map(|result| match result {
            Ok(output) => stream_query_output(output),
            Err(error) => stream::once(future::ready(Err(error))).boxed(),
        })
        .flatten()
        .boxed()
    }

    fn fetch_optional<'e, 'q, E>(
        self,
        mut query: E,
    ) -> BoxFuture<'e, Result<Option<MssqlRow>, Error>>
    where
        'c: 'e,
        E: Execute<'q, Self::Database>,
        'q: 'e,
        E: 'q,
    {
        let arguments = query.take_arguments().map_err(Error::Encode);
        let sql = query.sql();

        Box::pin(async move {
            let arguments = arguments?;
            Ok(self
                .run_execute_sql(sql.as_str(), arguments.as_ref())
                .await?
                .rows
                .into_iter()
                .next())
        })
    }

    fn prepare_with<'e>(
        self,
        sql: sqlx_core::sql_str::SqlStr,
        parameters: &'e [crate::MssqlTypeInfo],
    ) -> BoxFuture<'e, Result<MssqlStatement, Error>>
    where
        'c: 'e,
    {
        Box::pin(async move {
            let output = self.run_prepare(sql.as_str(), parameters).await?;
            let parameters = if parameters.is_empty() {
                None
            } else {
                Some(Either::Left(parameters.to_vec()))
            };

            Ok(MssqlStatement::with_parameters(
                sql,
                output.columns,
                parameters,
            ))
        })
    }
}

fn first_i32_return_value(output: &QueryOutput) -> Result<Option<i32>, Error> {
    output
        .return_values
        .first()
        .map(|value| {
            <i32 as Decode<Mssql>>::decode(value.as_ref()).map_err(|error| Error::ColumnDecode {
                index: "return value".to_owned(),
                source: error,
            })
        })
        .transpose()
}

pub(crate) fn wire_not_implemented() -> Error {
    Error::Protocol("SQL Server connection stream is not available".to_owned())
}

struct MssqlWireStream {
    stream: MssqlStream,
    packet_size: usize,
}

impl std::fmt::Debug for MssqlWireStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MssqlWireStream")
            .field("encrypted", &matches!(self.stream, MssqlStream::Tls(_)))
            .field("packet_size", &self.packet_size)
            .finish()
    }
}

enum MssqlStream {
    Raw(TcpStream),
    Tls(tokio_native_tls::TlsStream<TlsPreloginStream<TcpStream>>),
    Taken,
}

impl MssqlWireStream {
    async fn connect(options: &MssqlConnectOptions) -> Result<Self, Error> {
        let port = match (options.port(), options.instance()) {
            (Some(port), _) => port,
            (None, Some(instance)) => ssrp::resolve_instance_port(options.host(), instance).await?,
            (None, None) => 1433,
        };

        let stream = TcpStream::connect((options.host(), port)).await?;
        let packet_size = usize::try_from(options.requested_packet_size()).map_err(|_| {
            Error::Protocol(format!(
                "SQL Server packet size {} does not fit usize",
                options.requested_packet_size()
            ))
        })?;

        Ok(Self {
            stream: MssqlStream::Raw(stream),
            packet_size,
        })
    }

    async fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        match &mut self.stream {
            MssqlStream::Raw(stream) => {
                write_tds_packets(stream, bytes).await?;
            }
            MssqlStream::Tls(stream) => {
                write_tds_packets(stream, bytes).await?;
            }
            MssqlStream::Taken => return Err(taken_stream_error()),
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        match &mut self.stream {
            MssqlStream::Raw(stream) => stream.shutdown().await?,
            MssqlStream::Tls(stream) => stream.shutdown().await?,
            MssqlStream::Taken => return Err(taken_stream_error()),
        }
        Ok(())
    }

    async fn enable_tls(&mut self, options: &MssqlConnectOptions) -> Result<(), Error> {
        let stream = match std::mem::replace(&mut self.stream, MssqlStream::Taken) {
            MssqlStream::Raw(stream) => stream,
            other => {
                self.stream = other;
                return Ok(());
            }
        };

        let mut stream = TlsPreloginStream::new(stream);
        stream.start_handshake();

        let domain = options
            .hostname_in_certificate()
            .unwrap_or_else(|| options.host());
        let connector = build_tls_connector(options)?;
        let mut stream = connector
            .connect(domain, stream)
            .await
            .map_err(|error| {
                Error::Tls(
                    std::io::Error::other(format!(
                        "SQL Server TLS handshake failed for host `{}` during the TDS PRELOGIN encryption upgrade \
                         (encrypt={:?}, trust_server_certificate={}, hostname_in_certificate={}, ssl_root_cert={}): {}",
                        domain,
                        options.encrypt(),
                        options.trust_server_certificate(),
                        options.hostname_in_certificate().unwrap_or("<not set>"),
                        options.ssl_root_cert().is_some(),
                        error
                    ))
                    .into(),
                )
            })?;
        stream.get_mut().get_mut().get_mut().finish_handshake();

        self.stream = MssqlStream::Tls(stream);
        Ok(())
    }

    async fn read_message(&mut self) -> Result<WireMessage, Error> {
        let mut packet_type = None;
        let mut expected_packet_id = None;
        let mut payload = Vec::new();

        loop {
            let mut header_bytes = [0u8; PACKET_HEADER_LEN];
            self.read_exact(&mut header_bytes).await?;
            let header = PacketHeader::decode(&header_bytes).map_err(packet_error)?;

            if let Some(packet_type) = packet_type {
                if header.packet_type != packet_type {
                    return Err(Error::Protocol(format!(
                        "mismatched SQL Server packet type: expected 0x{:02x}, got 0x{:02x}",
                        packet_type.code(),
                        header.packet_type.code()
                    )));
                }
            } else {
                packet_type = Some(header.packet_type);
            }

            if let Some(packet_id) = expected_packet_id {
                if header.packet_id != packet_id {
                    return Err(Error::Protocol(format!(
                        "non-contiguous SQL Server packet id: expected {packet_id}, got {}",
                        header.packet_id
                    )));
                }
            }

            let packet_len = usize::from(header.length);
            if packet_len > self.packet_size {
                return Err(Error::Protocol(format!(
                    "SQL Server packet length {packet_len} exceeds negotiated packet size {}",
                    self.packet_size
                )));
            }

            let payload_len = packet_len.checked_sub(PACKET_HEADER_LEN).ok_or_else(|| {
                Error::Protocol("SQL Server packet header length underflow".to_owned())
            })?;
            let old_len = payload.len();
            payload.resize(old_len + payload_len, 0);
            self.read_exact(&mut payload[old_len..]).await?;

            expected_packet_id = Some(header.packet_id.wrapping_add(1));

            if header.status == PacketStatus::END_OF_MESSAGE {
                return Ok(WireMessage {
                    packet_type: packet_type.expect("packet_type is set after first header"),
                    payload,
                });
            }
        }
    }

    async fn read_exact(&mut self, bytes: &mut [u8]) -> Result<(), Error> {
        match &mut self.stream {
            MssqlStream::Raw(stream) => {
                stream.read_exact(bytes).await?;
            }
            MssqlStream::Tls(stream) => {
                stream.read_exact(bytes).await?;
            }
            MssqlStream::Taken => return Err(taken_stream_error()),
        }

        Ok(())
    }
}

async fn write_tds_packets<S>(stream: &mut S, bytes: &[u8]) -> Result<(), Error>
where
    S: AsyncWrite + Unpin,
{
    let mut offset = 0usize;

    while offset < bytes.len() {
        let packet = tds_packet_slice(bytes, offset)?;
        stream.write_all(packet).await?;
        stream.flush().await?;
        offset += packet.len();
    }

    Ok(())
}

fn tds_packet_slice(bytes: &[u8], offset: usize) -> Result<&[u8], Error> {
    let header_end = offset
        .checked_add(PACKET_HEADER_LEN)
        .ok_or_else(|| Error::Protocol("SQL Server outbound packet offset overflow".to_owned()))?;
    let header_bytes = bytes.get(offset..header_end).ok_or_else(|| {
        Error::Protocol("SQL Server outbound packet buffer ended inside a header".to_owned())
    })?;
    let header = PacketHeader::decode(header_bytes).map_err(packet_error)?;
    let packet_len = usize::from(header.length);
    let packet_end = offset
        .checked_add(packet_len)
        .ok_or_else(|| Error::Protocol("SQL Server outbound packet length overflow".to_owned()))?;

    bytes.get(offset..packet_end).ok_or_else(|| {
        Error::Protocol("SQL Server outbound packet buffer ended inside a packet".to_owned())
    })
}

#[derive(Debug)]
struct WireMessage {
    packet_type: PacketType,
    payload: Vec<u8>,
}

fn negotiate_encryption(requested: Encrypt, server: Encrypt) -> std::result::Result<bool, Error> {
    match (requested, server) {
        (Encrypt::NotSupported, Encrypt::NotSupported | Encrypt::Off) => Ok(false),
        (Encrypt::NotSupported, Encrypt::On | Encrypt::Required) => Err(Error::Protocol(
            "SQL Server requires encryption, but the client URL requested encrypt=not_supported"
                .to_owned(),
        )),
        (Encrypt::Required, Encrypt::Off | Encrypt::NotSupported) => Err(Error::Tls(
            "SQL Server TLS encryption is required but not supported by the server".into(),
        )),
        (Encrypt::On | Encrypt::Required, Encrypt::On | Encrypt::Required) => Ok(true),
        (Encrypt::Off, _) | (_, Encrypt::Off) => Err(Error::Protocol(
            "SQL Server login-only TLS fallback is not implemented yet; use encrypt=mandatory or encrypt=strict for encrypted connections, or encrypt=not_supported for plaintext development servers"
                .to_owned(),
        )),
        (Encrypt::On, Encrypt::NotSupported) => Ok(false),
    }
}

fn build_tls_connector(options: &MssqlConnectOptions) -> Result<TlsConnector, Error> {
    let mut builder = native_tls::TlsConnector::builder();
    builder.danger_accept_invalid_certs(options.trust_server_certificate());
    builder.danger_accept_invalid_hostnames(options.hostname_in_certificate().is_none());

    if let Some(path) = options.ssl_root_cert() {
        let cert = std::fs::read(path).map_err(Error::Io)?;
        let cert = Certificate::from_pem(&cert)
            .or_else(|_| Certificate::from_der(&cert))
            .map_err(|error| Error::Tls(error.into()))?;
        builder.add_root_certificate(cert);
    }

    builder
        .build()
        .map(TlsConnector::from)
        .map_err(|error| Error::Tls(error.into()))
}

fn taken_stream_error() -> Error {
    Error::Protocol("SQL Server stream was used while TLS upgrade was in progress".to_owned())
}

fn packet_error(error: crate::protocol::packet::PacketHeaderError) -> Error {
    Error::Protocol(error.to_string())
}

fn pre_login_error(error: PreLoginError) -> Error {
    Error::Protocol(error.to_string())
}

fn login_error(error: Login7Error) -> Error {
    Error::Protocol(error.to_string())
}

fn token_error(error: TokenParseError) -> Error {
    Error::Protocol(error.to_string())
}

fn frame_error(error: crate::protocol::packet::PacketFrameError) -> Error {
    Error::Protocol(error.to_string())
}

fn stream_query_output(
    output: QueryOutput,
) -> BoxStream<'static, Result<Either<MssqlQueryResult, MssqlRow>, Error>> {
    stream::iter(
        output
            .rows
            .into_iter()
            .map(|row| Ok(Either::Right(row)))
            .chain(std::iter::once(Ok(Either::Left(output.result)))),
    )
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_full_tls_for_required_or_mandatory_encryption() {
        assert!(negotiate_encryption(Encrypt::On, Encrypt::On).unwrap());
        assert!(negotiate_encryption(Encrypt::Required, Encrypt::Required).unwrap());
    }

    #[test]
    fn allows_plaintext_only_when_explicitly_requested_and_supported() {
        assert!(!negotiate_encryption(Encrypt::NotSupported, Encrypt::Off).unwrap());
        assert!(negotiate_encryption(Encrypt::NotSupported, Encrypt::Required).is_err());
    }

    #[test]
    fn rejects_login_only_tls_fallback_until_downgrade_is_available() {
        assert!(negotiate_encryption(Encrypt::Off, Encrypt::On).is_err());
        assert!(negotiate_encryption(Encrypt::On, Encrypt::Off).is_err());
    }

    #[test]
    fn slices_encoded_outbound_packets_by_header_length() {
        let bytes = crate::protocol::packet::encode_message(PacketType::RPC, &[0; 11], 12).unwrap();

        let first = tds_packet_slice(&bytes, 0).unwrap();
        assert_eq!(12, first.len());

        let second = tds_packet_slice(&bytes, first.len()).unwrap();
        assert_eq!(12, second.len());

        let third = tds_packet_slice(&bytes, first.len() + second.len()).unwrap();
        assert_eq!(11, third.len());
    }

    #[test]
    fn rejects_truncated_outbound_packet() {
        let bytes = crate::protocol::packet::encode_message(PacketType::RPC, &[0; 11], 12).unwrap();
        let err = tds_packet_slice(&bytes[..bytes.len() - 1], 24).unwrap_err();

        assert!(err.to_string().contains("ended inside a packet"));
    }
}
