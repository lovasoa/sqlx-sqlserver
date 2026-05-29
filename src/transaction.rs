use sqlx_core::database::Database;
use sqlx_core::error::Error;
use sqlx_core::sql_str::SqlStr;
use sqlx_core::transaction::TransactionManager;

use crate::{Mssql, MssqlConnection};

/// SQL Server transaction manager.
pub struct MssqlTransactionManager;

impl TransactionManager for MssqlTransactionManager {
    type Database = Mssql;

    async fn begin(conn: &mut MssqlConnection, statement: Option<SqlStr>) -> Result<(), Error> {
        let sql = statement.unwrap_or_else(|| {
            if conn.transaction_depth() == 0 {
                SqlStr::from_static("BEGIN TRANSACTION")
            } else {
                SqlStr::from_static("SAVE TRANSACTION sqlx_savepoint")
            }
        });

        conn.run_sql_batch(sql.as_str()).await?;
        conn.increment_transaction_depth();
        Ok(())
    }

    async fn commit(conn: &mut MssqlConnection) -> Result<(), Error> {
        if conn.transaction_depth() == 1 {
            conn.run_sql_batch("COMMIT TRANSACTION").await?;
            conn.decrement_transaction_depth();
        } else if conn.transaction_depth() > 1 {
            conn.decrement_transaction_depth();
        }

        Ok(())
    }

    async fn rollback(conn: &mut MssqlConnection) -> Result<(), Error> {
        if conn.transaction_depth() == 1 {
            conn.run_sql_batch("ROLLBACK TRANSACTION").await?;
            conn.clear_transaction_depth();
        } else if conn.transaction_depth() > 1 {
            conn.run_sql_batch("ROLLBACK TRANSACTION sqlx_savepoint")
                .await?;
            conn.decrement_transaction_depth();
        }

        Ok(())
    }

    fn start_rollback(conn: &mut MssqlConnection) {
        conn.queue_rollback();
    }

    fn get_transaction_depth(conn: &<Self::Database as Database>::Connection) -> usize {
        conn.transaction_depth()
    }
}
