use std::fmt::{self, Write};

use sqlx_core::arguments::Arguments;
use sqlx_core::encode::Encode;
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;

use crate::Mssql;

/// SQL Server argument buffer skeleton.
#[derive(Debug, Default, Clone)]
pub struct MssqlArguments {
    len: usize,
}

impl MssqlArguments {
    /// Returns `true` when no arguments were added.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Arguments for MssqlArguments {
    type Database = Mssql;

    fn reserve(&mut self, _additional: usize, _size: usize) {}

    fn add<'t, T>(&mut self, _value: T) -> Result<(), BoxDynError>
    where
        T: Encode<'t, Self::Database> + Type<Self::Database>,
    {
        self.len += 1;
        Ok(())
    }

    fn len(&self) -> usize {
        self.len
    }

    fn format_placeholder<W: Write>(&self, writer: &mut W) -> fmt::Result {
        write!(writer, "@p{}", self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sql_server_style_placeholders() {
        let args = MssqlArguments { len: 3 };
        let mut out = String::new();

        args.format_placeholder(&mut out).unwrap();

        assert_eq!("@p3", out);
    }
}
