//! Independent Microsoft SQL Server driver crate for SQLx.
//!
//! This crate is intentionally outside the SQLx workspace and does not use
//! local `path` dependencies. The current port includes tested connection
//! option parsing, PRELOGIN/LOGIN7 handshake support, TDS-wrapped TLS handshake
//! support for encrypted SQL Server connections, SQL batch execution,
//! transaction batches, and RPC execution for stable scalar bind parameters.
//!
//! # Testing
//!
//! Fast tests do not require SQL Server:
//!
//! ```text
//! cargo test
//! ```
//!
//! Integration tests require `MSSQL_DATABASE_URL` and skip cleanly when it is
//! absent:
//!
//! ```text
//! MSSQL_DATABASE_URL='mssql://sa:Password123!@localhost:1433/master?encrypt=mandatory&trust_server_certificate=true' \
//! cargo test --features integration-tests --test mssql_smoke
//! ```

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(future_incompatible, rust_2018_idioms)]

pub mod any;
mod arguments;
mod column;
mod connection;
mod database;
#[cfg(feature = "migrate")]
mod migrate;
/// Connection option parsing and configuration for SQL Server.
pub mod options;
pub mod protocol;
mod query_result;
mod row;
mod ssrp;
mod statement;
mod tls;
mod transaction;
mod type_info;
mod value;

pub use arguments::MssqlArguments;
pub use column::MssqlColumn;
pub use connection::MssqlConnection;
pub use database::Mssql;
pub use options::{Encrypt, MssqlConnectOptions, MssqlInvalidOption};
pub use query_result::MssqlQueryResult;
pub use row::MssqlRow;
pub use statement::MssqlStatement;
pub use transaction::MssqlTransactionManager;
pub use type_info::{MssqlType, MssqlTypeInfo};
pub use value::{MssqlValue, MssqlValueRef};

/// An alias for [`Pool`][sqlx_core::pool::Pool], specialized for SQL Server.
pub type MssqlPool = sqlx_core::pool::Pool<Mssql>;

/// An alias for [`PoolOptions`][sqlx_core::pool::PoolOptions], specialized for SQL Server.
pub type MssqlPoolOptions = sqlx_core::pool::PoolOptions<Mssql>;

/// An alias for [`Transaction`][sqlx_core::transaction::Transaction], specialized for SQL Server.
pub type MssqlTransaction<'c> = sqlx_core::transaction::Transaction<'c, Mssql>;

/// An alias for [`Executor<'_, Database = Mssql>`][sqlx_core::executor::Executor].
pub trait MssqlExecutor<'c>: sqlx_core::executor::Executor<'c, Database = Mssql> {}
impl<'c, T> MssqlExecutor<'c> for T where T: sqlx_core::executor::Executor<'c, Database = Mssql> {}

sqlx_core::impl_into_arguments_for_arguments!(MssqlArguments);
sqlx_core::impl_encode_for_option!(Mssql);
sqlx_core::impl_acquire!(Mssql, MssqlConnection);
