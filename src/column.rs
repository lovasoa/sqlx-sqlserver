use sqlx_core::column::Column;

use crate::{Mssql, MssqlTypeInfo};

/// SQL Server column metadata skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MssqlColumn {
    ordinal: usize,
    name: String,
    type_info: MssqlTypeInfo,
}

impl MssqlColumn {
    /// Creates column metadata for tests and future protocol plumbing.
    pub fn new(ordinal: usize, name: impl Into<String>, type_info: MssqlTypeInfo) -> Self {
        Self {
            ordinal,
            name: name.into(),
            type_info,
        }
    }
}

impl Column for MssqlColumn {
    type Database = Mssql;

    fn ordinal(&self) -> usize {
        self.ordinal
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn type_info(&self) -> &MssqlTypeInfo {
        &self.type_info
    }
}
