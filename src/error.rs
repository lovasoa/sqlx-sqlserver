//! SQL Server database errors.

use crate::protocol::token::ServerError;
use sqlx_core::error::{DatabaseError, ErrorKind};
use sqlx_core::Error;

/// An error reported by SQL Server.
#[derive(Debug)]
pub struct MssqlDatabaseError(pub(crate) ServerError);

impl MssqlDatabaseError {
    /// SQL Server error number.
    pub fn number(&self) -> i32 {
        self.0.number
    }

    /// SQL Server error state.
    pub fn state(&self) -> u8 {
        self.0.state
    }

    /// SQL Server error class, also known as severity.
    pub fn class(&self) -> u8 {
        self.0.class
    }

    /// Human-readable error message returned by SQL Server.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// Server name reported with the error.
    pub fn server_name(&self) -> &str {
        &self.0.server_name
    }

    /// Stored procedure name reported with the error, if any.
    pub fn procedure_name(&self) -> &str {
        &self.0.procedure_name
    }

    /// Line number reported by SQL Server.
    pub fn line_number(&self) -> u32 {
        self.0.line_number
    }
}

impl std::fmt::Display for MssqlDatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SQL Server error {} (state {}, class {}): {}",
            self.0.number, self.0.state, self.0.class, self.0.message
        )
    }
}

impl std::error::Error for MssqlDatabaseError {}

impl DatabaseError for MssqlDatabaseError {
    fn message(&self) -> &str {
        self.message()
    }

    fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self
    }

    fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
        self
    }

    fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
        self
    }

    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

pub(crate) fn server_error(error: ServerError) -> Error {
    Error::database(MssqlDatabaseError(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_error_returns_database_error() {
        let error = server_error(ServerError {
            number: 18456,
            state: 1,
            class: 14,
            message: "Login failed".to_owned(),
            server_name: "dbhost".to_owned(),
            procedure_name: "login_proc".to_owned(),
            line_number: 9,
        });

        let db_error = error.as_database_error().unwrap();
        let mssql_error = db_error
            .as_error()
            .downcast_ref::<MssqlDatabaseError>()
            .unwrap();

        assert_eq!(18456, mssql_error.number());
        assert_eq!(1, mssql_error.state());
        assert_eq!(14, mssql_error.class());
        assert_eq!("Login failed", mssql_error.message());
        assert_eq!("dbhost", mssql_error.server_name());
        assert_eq!("login_proc", mssql_error.procedure_name());
        assert_eq!(9, mssql_error.line_number());
        assert_eq!(None, db_error.code());
        assert_eq!(ErrorKind::Other, db_error.kind());
    }
}
