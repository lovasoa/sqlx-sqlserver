//! Runtime `Any` driver registration for SQL Server.
//!
//! The driver can be installed with SQLx `Any`. The current SQL Server port supports the same
//! limited SQL batch execution path as the native connection and rejects bound arguments until RPC
//! execution is ported.

use crate::{
    connection::wire_not_implemented, Mssql, MssqlColumn, MssqlConnectOptions, MssqlConnection,
    MssqlQueryResult, MssqlTransactionManager, MssqlType, MssqlTypeInfo,
};
use futures_core::future::BoxFuture;
use futures_core::stream::BoxStream;
use futures_util::{future, stream, FutureExt, StreamExt};
use sqlx_core::any::driver::AnyDriver;
use sqlx_core::any::{
    AnyArguments, AnyColumn, AnyConnectOptions, AnyConnectionBackend, AnyQueryResult, AnyRow,
    AnyStatement, AnyTypeInfo, AnyTypeInfoKind,
};
use sqlx_core::arguments::Arguments;
use sqlx_core::column::Column;
use sqlx_core::connection::{ConnectOptions, Connection};
use sqlx_core::database::Database;
use sqlx_core::ext::ustr::UStr;
use sqlx_core::row::Row;
use sqlx_core::sql_str::SqlStr;
use sqlx_core::transaction::TransactionManager;
use sqlx_core::{Either, Error, HashMap};
use std::sync::Arc;

/// Installable SQL Server driver for SQLx `Any` connections.
pub const DRIVER: AnyDriver = AnyDriver::without_migrate::<Mssql>();

impl AnyConnectionBackend for MssqlConnection {
    fn name(&self) -> &str {
        <Mssql as Database>::NAME
    }

    fn close(self: Box<Self>) -> BoxFuture<'static, sqlx_core::Result<()>> {
        Connection::close(*self).boxed()
    }

    fn close_hard(self: Box<Self>) -> BoxFuture<'static, sqlx_core::Result<()>> {
        Connection::close_hard(*self).boxed()
    }

    fn ping(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        Connection::ping(self).boxed()
    }

    fn begin(&mut self, statement: Option<SqlStr>) -> BoxFuture<'_, sqlx_core::Result<()>> {
        MssqlTransactionManager::begin(self, statement).boxed()
    }

    fn commit(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        MssqlTransactionManager::commit(self).boxed()
    }

    fn rollback(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        MssqlTransactionManager::rollback(self).boxed()
    }

    fn start_rollback(&mut self) {
        MssqlTransactionManager::start_rollback(self);
    }

    fn get_transaction_depth(&self) -> usize {
        MssqlTransactionManager::get_transaction_depth(self)
    }

    fn shrink_buffers(&mut self) {
        Connection::shrink_buffers(self);
    }

    fn flush(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        Connection::flush(self).boxed()
    }

    fn should_flush(&self) -> bool {
        Connection::should_flush(self)
    }

    fn fetch_many(
        &mut self,
        query: SqlStr,
        _persistent: bool,
        arguments: Option<AnyArguments>,
    ) -> BoxStream<'_, sqlx_core::Result<Either<AnyQueryResult, AnyRow>>> {
        stream::once(async move {
            reject_any_arguments(arguments.as_ref())?;
            self.run_sql_batch(query.as_str()).await
        })
        .map(|result| match result {
            Ok(output) => {
                let rows = output.rows.into_iter().map(|row| {
                    let column_names = column_names(row.columns());
                    AnyRow::map_from(&row, column_names).map(Either::Right)
                });
                let done = std::iter::once(Ok(Either::Left(map_result(output.result))));
                stream::iter(rows.chain(done)).boxed()
            }
            Err(error) => stream::once(future::ready(Err(error))).boxed(),
        })
        .flatten()
        .boxed()
    }

    fn fetch_optional(
        &mut self,
        query: SqlStr,
        _persistent: bool,
        arguments: Option<AnyArguments>,
    ) -> BoxFuture<'_, sqlx_core::Result<Option<AnyRow>>> {
        Box::pin(async move {
            reject_any_arguments(arguments.as_ref())?;
            self.run_sql_batch(query.as_str())
                .await?
                .rows
                .into_iter()
                .next()
                .map(|row| {
                    let column_names = column_names(row.columns());
                    AnyRow::map_from(&row, column_names)
                })
                .transpose()
        })
    }

    fn prepare_with<'c, 'q: 'c>(
        &'c mut self,
        _sql: SqlStr,
        _parameters: &[AnyTypeInfo],
    ) -> BoxFuture<'c, sqlx_core::Result<AnyStatement>> {
        Box::pin(future::ready(Err(wire_not_implemented())))
    }
}

fn reject_any_arguments(arguments: Option<&AnyArguments>) -> Result<(), Error> {
    if arguments.is_some_and(|arguments| arguments.len() > 0) {
        return Err(Error::Protocol(
            "SQL Server Any RPC execution for bound arguments is not implemented yet".to_owned(),
        ));
    }

    Ok(())
}

fn map_result(result: MssqlQueryResult) -> AnyQueryResult {
    AnyQueryResult {
        rows_affected: result.rows_affected(),
        last_insert_id: None,
    }
}

fn column_names(columns: &[MssqlColumn]) -> Arc<HashMap<UStr, usize>> {
    Arc::new(
        columns
            .iter()
            .map(|column| (UStr::new(column.name()), column.ordinal()))
            .collect(),
    )
}

impl<'a> TryFrom<&'a AnyConnectOptions> for MssqlConnectOptions {
    type Error = Error;

    fn try_from(options: &'a AnyConnectOptions) -> Result<Self, Self::Error> {
        MssqlConnectOptions::from_url(&options.database_url)
    }
}

impl<'a> TryFrom<&'a MssqlTypeInfo> for AnyTypeInfo {
    type Error = Error;

    fn try_from(type_info: &'a MssqlTypeInfo) -> Result<Self, Self::Error> {
        let kind = match type_info.kind() {
            MssqlType::Null => AnyTypeInfoKind::Null,
            MssqlType::Bit => AnyTypeInfoKind::Bool,
            MssqlType::SmallInt => AnyTypeInfoKind::SmallInt,
            MssqlType::Int => AnyTypeInfoKind::Integer,
            MssqlType::BigInt => AnyTypeInfoKind::BigInt,
            MssqlType::Real => AnyTypeInfoKind::Real,
            MssqlType::Float => AnyTypeInfoKind::Double,
            MssqlType::NVarChar => AnyTypeInfoKind::Text,
            MssqlType::VarBinary => AnyTypeInfoKind::Blob,
            MssqlType::TinyInt | MssqlType::Other(_) => {
                return Err(Error::AnyDriverError(
                    format!("Any driver does not support the SQL Server type {type_info:?}").into(),
                ));
            }
        };

        Ok(AnyTypeInfo { kind })
    }
}

impl<'a> TryFrom<&'a MssqlColumn> for AnyColumn {
    type Error = Error;

    fn try_from(column: &'a MssqlColumn) -> Result<Self, Self::Error> {
        let type_info =
            AnyTypeInfo::try_from(column.type_info()).map_err(|error| Error::ColumnDecode {
                index: column.name().to_owned(),
                source: error.into(),
            })?;

        Ok(Self {
            ordinal: column.ordinal(),
            name: UStr::new(column.name()),
            type_info,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_stable_sql_server_types_to_any_types() {
        assert_eq!(
            AnyTypeInfo::try_from(&MssqlTypeInfo::BIT).unwrap().kind(),
            AnyTypeInfoKind::Bool
        );
        assert_eq!(
            AnyTypeInfo::try_from(&MssqlTypeInfo::INT).unwrap().kind(),
            AnyTypeInfoKind::Integer
        );
        assert_eq!(
            AnyTypeInfo::try_from(&MssqlTypeInfo::NVARCHAR)
                .unwrap()
                .kind(),
            AnyTypeInfoKind::Text
        );
        assert_eq!(
            AnyTypeInfo::try_from(&MssqlTypeInfo::VARBINARY)
                .unwrap()
                .kind(),
            AnyTypeInfoKind::Blob
        );
    }

    #[test]
    fn rejects_unstable_sql_server_types_for_any_mapping() {
        assert!(matches!(
            AnyTypeInfo::try_from(&MssqlTypeInfo::TINYINT),
            Err(Error::AnyDriverError(_))
        ));
    }
}
