use futures_core::future::BoxFuture;
use futures_core::stream::BoxStream;
use futures_util::{future, stream, StreamExt};
use sqlx_core::connection::Connection;
use sqlx_core::decode::Decode;
use sqlx_core::error::Error;
use sqlx_core::executor::{Execute, Executor};
use sqlx_core::transaction::Transaction;
use sqlx_core::value::Value;
use sqlx_core::Either;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::protocol::login::{build_login7_packet, Login7Error};
use crate::protocol::packet::{PacketHeader, PacketStatus, PacketType, PACKET_HEADER_LEN};
use crate::protocol::pre_login::{build_pre_login_packet, parse_server_encrypt, PreLoginError};
use crate::protocol::query::{build_sql_batch_packet, parse_query_response, QueryOutput};
use crate::protocol::rpc::{
    build_execute_sql_packet, build_prepare_packet, build_unprepare_packet,
};
use crate::protocol::token::{parse_login_response, LoginResponse, ServerError, TokenParseError};
use crate::{
    ssrp, Encrypt, Mssql, MssqlArguments, MssqlConnectOptions, MssqlQueryResult, MssqlRow,
    MssqlStatement, MssqlTypeInfo,
};

/// SQL Server connection.
#[derive(Debug)]
pub struct MssqlConnection {
    stream: Option<MssqlWireStream>,
    transaction_depth: usize,
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
        validate_unencrypted_login(options.encrypt(), server_encrypt)?;

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
            LoginResponse::Success { .. } => Ok(Self {
                stream: Some(stream),
                transaction_depth: 0,
            }),
            LoginResponse::ServerError(error) => Err(server_error(error)),
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
        let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
        let packet = build_sql_batch_packet(sql, stream.packet_size, 0).map_err(frame_error)?;
        stream.write_all(&packet).await?;

        self.read_query_response().await
    }

    pub(crate) async fn run_execute_sql(
        &mut self,
        sql: &str,
        arguments: Option<&MssqlArguments>,
    ) -> Result<QueryOutput, Error> {
        match arguments {
            Some(arguments) if !arguments.is_empty() => {
                let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
                let packet = build_execute_sql_packet(sql, arguments, stream.packet_size, 0)
                    .map_err(|error| {
                        Error::Protocol(format!("failed to encode SQL Server RPC: {error}"))
                    })?;
                stream.write_all(&packet).await?;
                self.read_query_response().await
            }
            _ => self.run_sql_batch(sql).await,
        }
    }

    pub(crate) async fn run_prepare(
        &mut self,
        sql: &str,
        parameters: &[MssqlTypeInfo],
    ) -> Result<QueryOutput, Error> {
        let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
        let packet =
            build_prepare_packet(sql, parameters, stream.packet_size, 0).map_err(|error| {
                Error::Protocol(format!("failed to encode SQL Server prepare RPC: {error}"))
            })?;
        stream.write_all(&packet).await?;

        let output = self.read_query_response().await?;

        if let Some(statement_id) = first_i32_return_value(&output)? {
            let stream = self.stream.as_mut().ok_or_else(wire_not_implemented)?;
            let packet =
                build_unprepare_packet(statement_id, stream.packet_size, 0).map_err(|error| {
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

        parse_query_response(&response.payload)
    }
}

impl Connection for MssqlConnection {
    type Database = Mssql;
    type Options = MssqlConnectOptions;

    async fn close(mut self) -> Result<(), Error> {
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
    Error::Protocol("SQL Server query execution is not implemented in this port slice".to_owned())
}

#[derive(Debug)]
struct MssqlWireStream {
    stream: TcpStream,
    packet_size: usize,
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
            stream,
            packet_size,
        })
    }

    async fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.stream.write_all(bytes).await?;
        self.stream.flush().await?;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<(), Error> {
        self.stream.shutdown().await?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<WireMessage, Error> {
        let mut packet_type = None;
        let mut expected_packet_id = None;
        let mut payload = Vec::new();

        loop {
            let mut header_bytes = [0u8; PACKET_HEADER_LEN];
            self.stream.read_exact(&mut header_bytes).await?;
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
            self.stream.read_exact(&mut payload[old_len..]).await?;

            expected_packet_id = Some(header.packet_id.wrapping_add(1));

            if header.status == PacketStatus::END_OF_MESSAGE {
                return Ok(WireMessage {
                    packet_type: packet_type.expect("packet_type is set after first header"),
                    payload,
                });
            }
        }
    }
}

#[derive(Debug)]
struct WireMessage {
    packet_type: PacketType,
    payload: Vec<u8>,
}

fn validate_unencrypted_login(
    requested: Encrypt,
    server: Encrypt,
) -> std::result::Result<(), Error> {
    match (requested, server) {
        (Encrypt::NotSupported, Encrypt::NotSupported | Encrypt::Off) => Ok(()),
        (Encrypt::NotSupported, Encrypt::On | Encrypt::Required) => Err(Error::Protocol(
            "SQL Server requires encrypted login, but TLS is not implemented in this port slice"
                .to_owned(),
        )),
        _ => Err(Error::Protocol(
            "SQL Server TLS pre-login is not implemented yet; use encrypt=not_supported only with servers that allow unencrypted login"
                .to_owned(),
        )),
    }
}

fn server_error(error: ServerError) -> Error {
    Error::Protocol(format!(
        "SQL Server error {} (state {}, class {}): {}",
        error.number, error.state, error.class, error.message
    ))
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
