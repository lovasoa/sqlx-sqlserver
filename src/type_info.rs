use std::fmt::{self, Display, Formatter};

use sqlx_core::type_info::TypeInfo;

/// SQL Server scalar type families known by the skeleton driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MssqlType {
    /// SQL `NULL`.
    Null,
    /// SQL Server `bit`.
    Bit,
    /// SQL Server `tinyint`.
    TinyInt,
    /// SQL Server `smallint`.
    SmallInt,
    /// SQL Server `int`.
    Int,
    /// SQL Server `bigint`.
    BigInt,
    /// SQL Server `real`.
    Real,
    /// SQL Server `float`.
    Float,
    /// Unicode text such as `nvarchar`.
    NVarChar,
    /// Binary data such as `varbinary`.
    VarBinary,
    /// A type not yet mapped by this skeleton.
    Other(String),
}

/// SQL Server type information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MssqlTypeInfo {
    kind: MssqlType,
}

impl MssqlTypeInfo {
    /// Creates type information from a known SQL Server type family.
    pub const fn new(kind: MssqlType) -> Self {
        Self { kind }
    }

    /// Returns the known SQL Server type family.
    pub fn kind(&self) -> &MssqlType {
        &self.kind
    }

    /// SQL `NULL` type information.
    pub const NULL: Self = Self::new(MssqlType::Null);
    /// SQL Server `bit` type information.
    pub const BIT: Self = Self::new(MssqlType::Bit);
    /// SQL Server `tinyint` type information.
    pub const TINYINT: Self = Self::new(MssqlType::TinyInt);
    /// SQL Server `smallint` type information.
    pub const SMALLINT: Self = Self::new(MssqlType::SmallInt);
    /// SQL Server `int` type information.
    pub const INT: Self = Self::new(MssqlType::Int);
    /// SQL Server `bigint` type information.
    pub const BIGINT: Self = Self::new(MssqlType::BigInt);
    /// SQL Server `real` type information.
    pub const REAL: Self = Self::new(MssqlType::Real);
    /// SQL Server `float` type information.
    pub const FLOAT: Self = Self::new(MssqlType::Float);
    /// SQL Server Unicode text type information.
    pub const NVARCHAR: Self = Self::new(MssqlType::NVarChar);
    /// SQL Server binary type information.
    pub const VARBINARY: Self = Self::new(MssqlType::VarBinary);
}

impl TypeInfo for MssqlTypeInfo {
    fn is_null(&self) -> bool {
        matches!(self.kind, MssqlType::Null)
    }

    fn name(&self) -> &str {
        match &self.kind {
            MssqlType::Null => "NULL",
            MssqlType::Bit => "BIT",
            MssqlType::TinyInt => "TINYINT",
            MssqlType::SmallInt => "SMALLINT",
            MssqlType::Int => "INT",
            MssqlType::BigInt => "BIGINT",
            MssqlType::Real => "REAL",
            MssqlType::Float => "FLOAT",
            MssqlType::NVarChar => "NVARCHAR",
            MssqlType::VarBinary => "VARBINARY",
            MssqlType::Other(name) => name,
        }
    }

    fn type_compatible(&self, other: &Self) -> bool {
        self == other || self.is_null() || other.is_null()
    }
}

impl Display for MssqlTypeInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_sql_server_type_names() {
        assert_eq!("INT", MssqlTypeInfo::INT.name());
        assert_eq!("NVARCHAR", MssqlTypeInfo::NVARCHAR.to_string());
    }

    #[test]
    fn null_is_compatible_with_known_types() {
        assert!(MssqlTypeInfo::NULL.type_compatible(&MssqlTypeInfo::INT));
        assert!(MssqlTypeInfo::NVARCHAR.type_compatible(&MssqlTypeInfo::NULL));
        assert!(!MssqlTypeInfo::INT.type_compatible(&MssqlTypeInfo::BIGINT));
    }
}
