//! Microsoft SQL Server driver for SQLx.
//!
//! `sqlx-sqlserver` is an independent split driver crate. Use it directly with
//! `sqlx-core` native APIs, or install its [`Any` driver][any::DRIVER] when an
//! application wants to open SQL Server URLs through `AnyConnection`.
//!
//! # Native connection
//!
//! ```no_run
//! use sqlx_core::connection::{ConnectOptions, Connection};
//! use sqlx_core::row::Row;
//! use sqlx_sqlserver::MssqlConnectOptions;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let mut conn = "mssql://sa:Password123!@localhost:1433/master?encrypt=mandatory&trust_server_certificate=true"
//!     .parse::<MssqlConnectOptions>()?
//!     .connect()
//!     .await?;
//!
//! let row = sqlx_core::query::query("SELECT 1")
//!     .fetch_one(&mut conn)
//!     .await?;
//!
//! let value: i32 = row.try_get(0)?;
//! assert_eq!(value, 1);
//!
//! conn.close().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # `AnyConnection`
//!
//! Install this driver before connecting through SQLx `Any` APIs:
//!
//! ```no_run
//! use sqlx_core::connection::Connection;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! sqlx_core::any::driver::install_drivers(&[sqlx_sqlserver::any::DRIVER])?;
//!
//! let mut conn = sqlx_core::any::AnyConnection::connect(
//!     "mssql://sa:Password123!@localhost:1433/master?encrypt=mandatory&trust_server_certificate=true",
//! )
//! .await?;
//!
//! conn.close().await?;
//! # Ok(())
//! # }
//! ```
//!
//! To combine split drivers, install all of them once at application startup:
//!
//! ```ignore
//! fn install() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! sqlx_core::any::driver::install_drivers(&[
//!     sqlx_sqlserver::any::DRIVER,
//!     sqlx_odbc::any::DRIVER,
//! ])?;
//! Ok(())
//! }
//! ```
//!
//! The examples use `trust_server_certificate=true` for local development with
//! SQL Server's self-signed container certificate. Production deployments should
//! prefer a trusted certificate and, when needed, `hostname_in_certificate` or
//! `ssl_root_cert` in [`MssqlConnectOptions`].

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(future_incompatible, rust_2018_idioms)]

pub mod any;
mod arguments;
mod column;
mod connection;
mod database;
mod decimal_tools;
mod error;
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
mod types;
mod value;

pub use arguments::MssqlArguments;
pub use column::MssqlColumn;
pub use connection::MssqlConnection;
pub use database::Mssql;
pub use error::MssqlDatabaseError;
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
