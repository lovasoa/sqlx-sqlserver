use sqlx_core::database::Database;
use sqlx_core::error::Error;
use sqlx_core::sql_str::SqlStr;
use sqlx_core::transaction::TransactionManager;

use crate::{connection::wire_not_implemented, Mssql, MssqlConnection};

/// SQL Server transaction manager skeleton.
pub struct MssqlTransactionManager;

impl TransactionManager for MssqlTransactionManager {
    type Database = Mssql;

    async fn begin(_conn: &mut MssqlConnection, _statement: Option<SqlStr>) -> Result<(), Error> {
        Err(wire_not_implemented())
    }

    async fn commit(_conn: &mut MssqlConnection) -> Result<(), Error> {
        Err(wire_not_implemented())
    }

    async fn rollback(_conn: &mut MssqlConnection) -> Result<(), Error> {
        Err(wire_not_implemented())
    }

    fn start_rollback(_conn: &mut MssqlConnection) {}

    fn get_transaction_depth(conn: &<Self::Database as Database>::Connection) -> usize {
        conn.transaction_depth()
    }
}
