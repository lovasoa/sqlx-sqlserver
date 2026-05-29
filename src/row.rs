use sqlx_core::column::{Column, ColumnIndex};
use sqlx_core::error::Error;
use sqlx_core::row::Row;
use sqlx_core::value::Value;

use crate::{Mssql, MssqlColumn, MssqlValue, MssqlValueRef};

/// SQL Server row skeleton.
#[derive(Debug, Default, Clone)]
pub struct MssqlRow {
    columns: Vec<MssqlColumn>,
    values: Vec<MssqlValue>,
}

impl MssqlRow {
    /// Creates an empty row skeleton.
    pub fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn new(columns: Vec<MssqlColumn>, values: Vec<MssqlValue>) -> Self {
        Self { columns, values }
    }
}

impl Row for MssqlRow {
    type Database = Mssql;

    fn columns(&self) -> &[MssqlColumn] {
        &self.columns
    }

    fn try_get_raw<I>(&self, index: I) -> Result<MssqlValueRef<'_>, Error>
    where
        I: ColumnIndex<Self>,
    {
        let index = index.index(self)?;
        Ok(self.values[index].as_ref())
    }
}

impl ColumnIndex<MssqlRow> for usize {
    fn index(&self, row: &MssqlRow) -> Result<usize, Error> {
        if *self >= row.columns.len() {
            return Err(Error::ColumnIndexOutOfBounds {
                index: *self,
                len: row.columns.len(),
            });
        }

        Ok(*self)
    }
}

impl ColumnIndex<MssqlRow> for &'_ str {
    fn index(&self, row: &MssqlRow) -> Result<usize, Error> {
        row.columns
            .iter()
            .position(|column| column.name() == *self)
            .ok_or_else(|| Error::ColumnNotFound((*self).to_owned()))
    }
}
