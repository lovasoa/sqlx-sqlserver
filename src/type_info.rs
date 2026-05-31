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
    /// SQL Server `decimal`/`numeric`.
    Decimal,
    /// SQL Server `money`/`smallmoney`.
    Money,
    /// SQL Server `date`.
    Date,
    /// SQL Server `time`.
    Time,
    /// SQL Server legacy `datetime`/`smalldatetime`.
    DateTime,
    /// SQL Server `datetime2`.
    DateTime2,
    /// SQL Server `datetimeoffset`.
    DateTimeOffset,
    /// SQL Server `uniqueidentifier`.
    UniqueIdentifier,
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

    pub(crate) const fn with_size(kind: MssqlType, size: u16) -> Self {
        Self {
            kind,
            variable_length: true,
            size: Some(size),
            protocol_type_info: None,
        }
    }

    pub(crate) const fn with_protocol(
        kind: MssqlType,
        protocol_type_info: protocol::TypeInfo,
    ) -> Self {
        Self {
            kind,
            variable_length: true,
            size: Some(protocol_type_info.size as u16),
            protocol_type_info: Some(protocol_type_info),
        }
    }

    /// Returns the known SQL Server type family.
    pub fn kind(&self) -> &MssqlType {
        &self.kind
    }

    pub(crate) const fn size(&self) -> Option<u16> {
        self.size
    }

    pub(crate) const fn protocol_type_info(&self) -> Option<&protocol::TypeInfo> {
        self.protocol_type_info.as_ref()
    }

    pub(crate) const fn scale(&self) -> u8 {
        match &self.protocol_type_info {
            Some(protocol_type_info) => protocol_type_info.scale,
            None => 0,
        }
    }

    pub(crate) const fn precision(&self) -> u8 {
        match &self.protocol_type_info {
            Some(protocol_type_info) => protocol_type_info.precision,
            None => 0,
        }
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
    /// SQL Server decimal/numeric type information.
    pub const DECIMAL: Self = Self::with_protocol(
        MssqlType::Decimal,
        protocol::TypeInfo {
            ty: protocol::DataType::NumericN,
            size: 17,
            scale: 0,
            precision: 38,
            collation: None,
        },
    );
    /// SQL Server money type information.
    pub const MONEY: Self = Self::new(MssqlType::Money);
    /// SQL Server date type information.
    pub const DATE: Self = Self::with_protocol(
        MssqlType::Date,
        protocol::TypeInfo {
            ty: protocol::DataType::DateN,
            size: 3,
            scale: 0,
            precision: 10,
            collation: None,
        },
    );
    /// SQL Server time type information.
    pub const TIME: Self = Self::with_protocol(
        MssqlType::Time,
        protocol::TypeInfo {
            ty: protocol::DataType::TimeN,
            size: 5,
            scale: 7,
            precision: 0,
            collation: None,
        },
    );
    /// SQL Server datetime2 type information.
    pub const DATETIME2: Self = Self::with_protocol(
        MssqlType::DateTime2,
        protocol::TypeInfo {
            ty: protocol::DataType::DateTime2N,
            size: 8,
            scale: 7,
            precision: 0,
            collation: None,
        },
    );
    /// SQL Server datetimeoffset type information.
    pub const DATETIMEOFFSET: Self = Self::with_protocol(
        MssqlType::DateTimeOffset,
        protocol::TypeInfo {
            ty: protocol::DataType::DateTimeOffsetN,
            size: 10,
            scale: 7,
            precision: 34,
            collation: None,
        },
    );
    /// SQL Server uniqueidentifier type information.
    pub const UNIQUEIDENTIFIER: Self = Self::with_protocol(
        MssqlType::UniqueIdentifier,
        protocol::TypeInfo {
            ty: protocol::DataType::Guid,
            size: 16,
            scale: 0,
            precision: 0,
            collation: None,
        },
    );

    pub(crate) const fn decimal_with_scale(scale: u8) -> Self {
        Self::with_protocol(
            MssqlType::Decimal,
            protocol::TypeInfo {
                ty: protocol::DataType::NumericN,
                size: 17,
                scale,
                precision: 38,
                collation: None,
            },
        )
    }

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
            protocol::DataType::Decimal
            | protocol::DataType::DecimalN
            | protocol::DataType::Numeric
            | protocol::DataType::NumericN => MssqlType::Decimal,
            protocol::DataType::Money
            | protocol::DataType::MoneyN
            | protocol::DataType::SmallMoney => MssqlType::Money,
            protocol::DataType::DateN => MssqlType::Date,
            protocol::DataType::TimeN => MssqlType::Time,
            protocol::DataType::DateTime
            | protocol::DataType::DateTimeN
            | protocol::DataType::SmallDateTime => MssqlType::DateTime,
            protocol::DataType::DateTime2N => MssqlType::DateTime2,
            protocol::DataType::DateTimeOffsetN => MssqlType::DateTimeOffset,
            protocol::DataType::Guid => MssqlType::UniqueIdentifier,
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
            MssqlType::Decimal => "DECIMAL",
            MssqlType::Money => "MONEY",
            MssqlType::Date => "DATE",
            MssqlType::Time => "TIME",
            MssqlType::DateTime => "DATETIME",
            MssqlType::DateTime2 => "DATETIME2",
            MssqlType::DateTimeOffset => "DATETIMEOFFSET",
            MssqlType::UniqueIdentifier => "UNIQUEIDENTIFIER",
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
