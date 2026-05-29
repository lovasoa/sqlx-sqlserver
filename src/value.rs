use std::borrow::Cow;

use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;
use sqlx_core::value::{Value, ValueRef};

use crate::{Mssql, MssqlType, MssqlTypeInfo};

/// Owned SQL Server value skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MssqlValue {
    type_info: MssqlTypeInfo,
    data: Option<Vec<u8>>,
}

impl MssqlValue {
    /// Creates an owned value from type information and raw little-endian TDS bytes.
    pub(crate) fn new(type_info: MssqlTypeInfo, data: Option<Vec<u8>>) -> Self {
        Self { type_info, data }
    }

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

impl MssqlValueRef<'_> {
    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        self.data
    }
}

impl Type<Mssql> for i32 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::INT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.kind(),
            MssqlType::TinyInt | MssqlType::SmallInt | MssqlType::Int
        )
    }
}

impl Encode<'_, Mssql> for i32 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(IsNull::No)
    }
}

impl Type<Mssql> for bool {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::BIT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Bit)
    }
}

impl Encode<'_, Mssql> for bool {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.push(u8::from(*self));
        Ok(IsNull::No)
    }
}

impl Decode<'_, Mssql> for bool {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as bool".to_owned())?;

        match bytes {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err("cannot decode SQL Server bit as bool".into()),
        }
    }
}

impl Type<Mssql> for i16 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::SMALLINT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::TinyInt | MssqlType::SmallInt)
    }
}

impl Encode<'_, Mssql> for i16 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(IsNull::No)
    }
}

impl Decode<'_, Mssql> for i16 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as i16".to_owned())?;

        match bytes.len() {
            1 => Ok(i16::from(bytes[0])),
            2 => Ok(i16::from_le_bytes([bytes[0], bytes[1]])),
            len => Err(format!("cannot decode {len}-byte SQL Server integer as i16").into()),
        }
    }
}

impl Decode<'_, Mssql> for i32 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as i32".to_owned())?;

        match bytes.len() {
            1 => Ok(i32::from(bytes[0])),
            2 => Ok(i32::from(i16::from_le_bytes([bytes[0], bytes[1]]))),
            4 => Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            len => Err(format!("cannot decode {len}-byte SQL Server integer as i32").into()),
        }
    }
}

impl Type<Mssql> for i64 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::BIGINT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.kind(),
            MssqlType::TinyInt | MssqlType::SmallInt | MssqlType::Int | MssqlType::BigInt
        )
    }
}

impl Encode<'_, Mssql> for i64 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(IsNull::No)
    }
}

impl Decode<'_, Mssql> for i64 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as i64".to_owned())?;

        match bytes.len() {
            1 => Ok(i64::from(bytes[0])),
            2 => Ok(i64::from(i16::from_le_bytes([bytes[0], bytes[1]]))),
            4 => Ok(i64::from(i32::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
            8 => Ok(i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ])),
            len => Err(format!("cannot decode {len}-byte SQL Server integer as i64").into()),
        }
    }
}

impl Type<Mssql> for f32 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::REAL
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Real)
    }
}

impl Encode<'_, Mssql> for f32 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(IsNull::No)
    }
}

impl Decode<'_, Mssql> for f32 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as f32".to_owned())?;

        match bytes {
            [a, b, c, d] => Ok(f32::from_le_bytes([*a, *b, *c, *d])),
            _ => Err("cannot decode SQL Server real as f32".into()),
        }
    }
}

impl Type<Mssql> for f64 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::FLOAT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::Real | MssqlType::Float)
    }
}

impl Decode<'_, Mssql> for f64 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        match value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as f64".to_owned())?
        {
            [a, b, c, d] => Ok(f64::from(f32::from_le_bytes([*a, *b, *c, *d]))),
            [a, b, c, d, e, f, g, h] => Ok(f64::from_le_bytes([*a, *b, *c, *d, *e, *f, *g, *h])),
            _ => Err("cannot decode SQL Server float as f64".into()),
        }
    }
}

impl Encode<'_, Mssql> for f64 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(&self.to_le_bytes());
        Ok(IsNull::No)
    }
}

impl Type<Mssql> for str {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::NVARCHAR
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::NVarChar)
    }
}

impl Type<Mssql> for String {
    fn type_info() -> MssqlTypeInfo {
        <str as Type<Mssql>>::type_info()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        <str as Type<Mssql>>::compatible(ty)
    }
}

impl Encode<'_, Mssql> for str {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        for unit in self.encode_utf16() {
            buf.extend_from_slice(&unit.to_le_bytes());
        }

        Ok(IsNull::No)
    }
}

impl<'q> Encode<'q, Mssql> for &'q str {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <str as Encode<Mssql>>::encode_by_ref(*self, buf)
    }
}

impl Encode<'_, Mssql> for String {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <str as Encode<Mssql>>::encode_by_ref(self.as_str(), buf)
    }
}

impl Decode<'_, Mssql> for String {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as String".to_owned())?;

        if bytes.len() % 2 != 0 {
            return Err("cannot decode odd-length SQL Server UTF-16 text".into());
        }

        let units = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        Ok(String::from_utf16(&units)?)
    }
}

impl Type<Mssql> for [u8] {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::VARBINARY
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.kind(), MssqlType::VarBinary)
    }
}

impl Type<Mssql> for Vec<u8> {
    fn type_info() -> MssqlTypeInfo {
        <[u8] as Type<Mssql>>::type_info()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        <[u8] as Type<Mssql>>::compatible(ty)
    }
}

impl Encode<'_, Mssql> for [u8] {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(self);
        Ok(IsNull::No)
    }
}

impl<'q> Encode<'q, Mssql> for &'q [u8] {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <[u8] as Encode<Mssql>>::encode_by_ref(*self, buf)
    }
}

impl Encode<'_, Mssql> for Vec<u8> {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <[u8] as Encode<Mssql>>::encode_by_ref(self.as_slice(), buf)
    }
}

impl Decode<'_, Mssql> for Vec<u8> {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as bytes".to_owned())?
            .to_vec())
    }
}
