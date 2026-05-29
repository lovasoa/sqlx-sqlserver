use sqlx_core::connection::Connection;
use sqlx_core::error::Error;
use sqlx_core::transaction::Transaction;

use crate::{Mssql, MssqlConnectOptions};

/// SQL Server connection skeleton.
#[derive(Debug, Clone)]
pub struct MssqlConnection {
    transaction_depth: usize,
}

impl MssqlConnection {
    /// Returns the current transaction depth tracked by the skeleton connection.
    pub const fn transaction_depth(&self) -> usize {
        self.transaction_depth
    }
}

impl Connection for MssqlConnection {
    type Database = Mssql;
    type Options = MssqlConnectOptions;

    async fn close(self) -> Result<(), Error> {
        Ok(())
    }

    async fn close_hard(self) -> Result<(), Error> {
        Ok(())
    }

    async fn ping(&mut self) -> Result<(), Error> {
        Err(wire_not_implemented())
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

pub(crate) fn wire_not_implemented() -> Error {
    Error::Protocol("SQL Server wire connection is not implemented in this port slice".to_owned())
}
