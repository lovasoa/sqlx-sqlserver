use sqlx_core::column::{Column, ColumnIndex};
use sqlx_core::error::Error;
use sqlx_core::sql_str::SqlStr;
use sqlx_core::statement::Statement;
use sqlx_core::Either;

use crate::{Mssql, MssqlArguments, MssqlColumn, MssqlTypeInfo};

/// SQL Server prepared statement metadata skeleton.
#[derive(Debug, Clone)]
pub struct MssqlStatement {
    sql: SqlStr,
    columns: Vec<MssqlColumn>,
    parameters: Option<Either<Vec<MssqlTypeInfo>, usize>>,
}

impl MssqlStatement {
    /// Creates statement metadata for tests and future prepare support.
    pub fn new(sql: SqlStr, columns: Vec<MssqlColumn>) -> Self {
        Self {
            sql,
            columns,
            parameters: None,
        }
    }

    pub(crate) fn with_parameters(
        sql: SqlStr,
        columns: Vec<MssqlColumn>,
        parameters: Option<Either<Vec<MssqlTypeInfo>, usize>>,
    ) -> Self {
        Self {
            sql,
            columns,
            parameters,
        }
    }
}

impl Statement for MssqlStatement {
    type Database = Mssql;

    fn into_sql(self) -> SqlStr {
        self.sql
    }

    fn sql(&self) -> &SqlStr {
        &self.sql
    }

    fn parameters(&self) -> Option<Either<&[MssqlTypeInfo], usize>> {
        match &self.parameters {
            Some(Either::Left(parameters)) => Some(Either::Left(parameters)),
            Some(Either::Right(count)) => Some(Either::Right(*count)),
            None => None,
        }
    }

    fn columns(&self) -> &[MssqlColumn] {
        &self.columns
    }

    sqlx_core::impl_statement_query!(MssqlArguments);
}

impl ColumnIndex<MssqlStatement> for usize {
    fn index(&self, statement: &MssqlStatement) -> Result<usize, Error> {
        if *self >= statement.columns.len() {
            return Err(Error::ColumnIndexOutOfBounds {
                index: *self,
                len: statement.columns.len(),
            });
        }

        Ok(*self)
    }
}

impl ColumnIndex<MssqlStatement> for &'_ str {
    fn index(&self, statement: &MssqlStatement) -> Result<usize, Error> {
        statement
            .columns
            .iter()
            .position(|column| column.name() == *self)
            .ok_or_else(|| Error::ColumnNotFound((*self).to_owned()))
    }
}
