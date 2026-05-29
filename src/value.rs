use std::borrow::Cow;

use sqlx_core::value::{Value, ValueRef};

use crate::{Mssql, MssqlTypeInfo};

/// Owned SQL Server value skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MssqlValue {
    type_info: MssqlTypeInfo,
    data: Option<Vec<u8>>,
}

impl MssqlValue {
    /// Creates a `NULL` value with the supplied type information.
    pub fn null(type_info: MssqlTypeInfo) -> Self {
        Self {
            type_info,
            data: None,
        }
    }
}

impl Value for MssqlValue {
    type Database = Mssql;

    fn as_ref(&self) -> MssqlValueRef<'_> {
        MssqlValueRef {
            type_info: &self.type_info,
            data: self.data.as_deref(),
        }
    }

    fn type_info(&self) -> Cow<'_, MssqlTypeInfo> {
        Cow::Borrowed(&self.type_info)
    }

    fn is_null(&self) -> bool {
        self.data.is_none()
    }
}

/// Borrowed SQL Server value skeleton.
#[derive(Debug, Clone, Copy)]
pub struct MssqlValueRef<'r> {
    type_info: &'r MssqlTypeInfo,
    data: Option<&'r [u8]>,
}

impl<'r> ValueRef<'r> for MssqlValueRef<'r> {
    type Database = Mssql;

    fn to_owned(&self) -> MssqlValue {
        MssqlValue {
            type_info: self.type_info.clone(),
            data: self.data.map(ToOwned::to_owned),
        }
    }

    fn type_info(&self) -> Cow<'_, MssqlTypeInfo> {
        Cow::Borrowed(self.type_info)
    }

    fn is_null(&self) -> bool {
        self.data.is_none()
    }
}
