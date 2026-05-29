use std::fmt::{self, Display, Formatter};

use sqlx_core::type_info::TypeInfo;

use crate::protocol::type_info as protocol;

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
    /// Non-Unicode text such as `varchar`.
    VarChar,
    /// Binary data such as `varbinary`.
    VarBinary,
    /// A type not yet mapped by this skeleton.
    Other(String),
}

/// SQL Server type information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MssqlTypeInfo {
    kind: MssqlType,
    variable_length: bool,
    size: Option<u16>,
    protocol_type_info: Option<protocol::TypeInfo>,
}

impl MssqlTypeInfo {
    /// Creates type information from a known SQL Server type family.
    pub const fn new(kind: MssqlType) -> Self {
        Self {
            kind,
            variable_length: false,
            size: None,
            protocol_type_info: None,
        }
    }

    /// Returns the known SQL Server type family.
    pub fn kind(&self) -> &MssqlType {
        &self.kind
    }

    pub(crate) const fn protocol_type_info(&self) -> Option<&protocol::TypeInfo> {
        self.protocol_type_info.as_ref()
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
    /// SQL Server non-Unicode text type information.
    pub const VARCHAR: Self = Self::new(MssqlType::VarChar);
    /// SQL Server binary type information.
    pub const VARBINARY: Self = Self::new(MssqlType::VarBinary);

    pub(crate) fn from_protocol(type_info: &protocol::TypeInfo) -> Self {
        let kind = match type_info.ty {
            protocol::DataType::Null => MssqlType::Null,
            protocol::DataType::Bit | protocol::DataType::BitN => MssqlType::Bit,
            protocol::DataType::TinyInt => MssqlType::TinyInt,
            protocol::DataType::SmallInt => MssqlType::SmallInt,
            protocol::DataType::Int => MssqlType::Int,
            protocol::DataType::BigInt => MssqlType::BigInt,
            protocol::DataType::Real => MssqlType::Real,
            protocol::DataType::Float => MssqlType::Float,
            protocol::DataType::IntN => match type_info.size {
                1 => MssqlType::TinyInt,
                2 => MssqlType::SmallInt,
                4 => MssqlType::Int,
                8 => MssqlType::BigInt,
                _ => MssqlType::Other(type_info.name().to_owned()),
            },
            protocol::DataType::FloatN => match type_info.size {
                4 => MssqlType::Real,
                8 => MssqlType::Float,
                _ => MssqlType::Other(type_info.name().to_owned()),
            },
            protocol::DataType::NVarChar | protocol::DataType::NChar => MssqlType::NVarChar,
            protocol::DataType::VarChar
            | protocol::DataType::Char
            | protocol::DataType::BigVarChar
            | protocol::DataType::BigChar => MssqlType::VarChar,
            protocol::DataType::VarBinary
            | protocol::DataType::Binary
            | protocol::DataType::BigVarBinary
            | protocol::DataType::BigBinary => MssqlType::VarBinary,
            _ => MssqlType::Other(type_info.name().to_owned()),
        };

        Self {
            kind,
            variable_length: type_info.is_nullable_or_variable_length(),
            size: u16::try_from(type_info.size).ok(),
            protocol_type_info: Some(type_info.clone()),
        }
    }
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
            MssqlType::VarChar => "VARCHAR",
            MssqlType::VarBinary => "VARBINARY",
            MssqlType::Other(name) => name,
        }
    }

    fn type_compatible(&self, other: &Self) -> bool {
        self.kind == other.kind || self.is_null() || other.is_null()
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
        assert_eq!("VARCHAR", MssqlTypeInfo::VARCHAR.to_string());
    }

    #[test]
    fn null_is_compatible_with_known_types() {
        assert!(MssqlTypeInfo::NULL.type_compatible(&MssqlTypeInfo::INT));
        assert!(MssqlTypeInfo::NVARCHAR.type_compatible(&MssqlTypeInfo::NULL));
        assert!(!MssqlTypeInfo::INT.type_compatible(&MssqlTypeInfo::BIGINT));
    }

    #[test]
    fn maps_unicode_and_non_unicode_protocol_text_separately() {
        assert_eq!(
            MssqlType::NVarChar,
            MssqlTypeInfo::from_protocol(&protocol::TypeInfo::new(protocol::DataType::NVarChar, 8))
                .kind
        );
        assert_eq!(
            MssqlType::VarChar,
            MssqlTypeInfo::from_protocol(&protocol::TypeInfo::new(protocol::DataType::VarChar, 8))
                .kind
        );
        assert_eq!(
            MssqlType::VarChar,
            MssqlTypeInfo::from_protocol(&protocol::TypeInfo::new(
                protocol::DataType::BigVarChar,
                8,
            ))
            .kind
        );
    }
}
