use sqlx_core::database::Database;

use crate::{
    MssqlArguments, MssqlColumn, MssqlConnection, MssqlQueryResult, MssqlRow, MssqlStatement,
    MssqlTransactionManager, MssqlTypeInfo, MssqlValue, MssqlValueRef,
};

/// SQL Server database driver marker.
#[derive(Debug)]
pub struct Mssql;

impl Database for Mssql {
    type Connection = MssqlConnection;
    type TransactionManager = MssqlTransactionManager;
    type Row = MssqlRow;
    type QueryResult = MssqlQueryResult;
    type Column = MssqlColumn;
    type TypeInfo = MssqlTypeInfo;
    type Value = MssqlValue;
    type ValueRef<'r> = MssqlValueRef<'r>;
    type Arguments = MssqlArguments;
    type ArgumentBuffer = Vec<u8>;
    type Statement = MssqlStatement;

    const NAME: &'static str = "Microsoft SQL Server";
    const URL_SCHEMES: &'static [&'static str] = &["mssql", "sqlserver"];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_sqlx_database_metadata() {
        assert_eq!("Microsoft SQL Server", <Mssql as Database>::NAME);
        assert_eq!(&["mssql", "sqlserver"], <Mssql as Database>::URL_SCHEMES);
    }
}
