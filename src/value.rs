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

impl<'r> MssqlValueRef<'r> {
    pub(crate) fn as_bytes(&self) -> Option<&'r [u8]> {
        self.data
    }
}

fn non_null_bytes<'r>(value: MssqlValueRef<'r>, rust_type: &str) -> Result<&'r [u8], BoxDynError> {
    value
        .as_bytes()
        .ok_or_else(|| format!("cannot decode SQL Server NULL as {rust_type}").into())
}

fn decode_integer(value: MssqlValueRef<'_>, rust_type: &str) -> Result<i64, BoxDynError> {
    let bytes = non_null_bytes(value, rust_type)?;

    match bytes.len() {
        1 => Ok(i64::from(bytes[0])),
        2 => Ok(i64::from(i16::from_le_bytes([bytes[0], bytes[1]]))),
        4 => Ok(i64::from(i32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        8 => Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])),
        len => Err(format!("cannot decode {len}-byte SQL Server integer as {rust_type}").into()),
    }
}

impl Type<Mssql> for i8 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::SMALLINT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.kind(),
            MssqlType::TinyInt | MssqlType::SmallInt | MssqlType::Int | MssqlType::BigInt
        )
    }
}

impl Encode<'_, Mssql> for i8 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <i16 as Encode<Mssql>>::encode_by_ref(&i16::from(*self), buf)
    }
}

impl Decode<'_, Mssql> for i8 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(i8::try_from(decode_integer(value, "i8")?)?)
    }
}

impl Type<Mssql> for u8 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::TINYINT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.kind(),
            MssqlType::TinyInt | MssqlType::SmallInt | MssqlType::Int | MssqlType::BigInt
        )
    }
}

impl Encode<'_, Mssql> for u8 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.push(*self);
        Ok(IsNull::No)
    }
}

impl Decode<'_, Mssql> for u8 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(u8::try_from(decode_integer(value, "u8")?)?)
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
        Ok(i16::try_from(decode_integer(value, "i16")?)?)
    }
}

impl Type<Mssql> for u16 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::INT
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.kind(),
            MssqlType::TinyInt | MssqlType::SmallInt | MssqlType::Int | MssqlType::BigInt
        )
    }
}

impl Encode<'_, Mssql> for u16 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <i32 as Encode<Mssql>>::encode_by_ref(&i32::from(*self), buf)
    }
}

impl Decode<'_, Mssql> for u16 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(u16::try_from(decode_integer(value, "u16")?)?)
    }
}

impl Decode<'_, Mssql> for i32 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(i32::try_from(decode_integer(value, "i32")?)?)
    }
}

impl Type<Mssql> for u32 {
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

impl Encode<'_, Mssql> for u32 {
    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <i64 as Encode<Mssql>>::encode_by_ref(&i64::from(*self), buf)
    }
}

impl Decode<'_, Mssql> for u32 {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(u32::try_from(decode_integer(value, "u32")?)?)
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
        decode_integer(value, "i64")
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
        matches!(ty.kind(), MssqlType::NVarChar | MssqlType::VarChar)
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
    fn produces(&self) -> Option<MssqlTypeInfo> {
        Some(MssqlTypeInfo::with_size(
            MssqlType::NVarChar,
            nvarchar_parameter_size(self),
        ))
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        for unit in self.encode_utf16() {
            buf.extend_from_slice(&unit.to_le_bytes());
        }

        Ok(IsNull::No)
    }
}

impl<'q> Encode<'q, Mssql> for &'q str {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        <str as Encode<Mssql>>::produces(*self)
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <str as Encode<Mssql>>::encode_by_ref(*self, buf)
    }
}

impl Encode<'_, Mssql> for String {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        <str as Encode<Mssql>>::produces(self.as_str())
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <str as Encode<Mssql>>::encode_by_ref(self.as_str(), buf)
    }
}

impl Decode<'_, Mssql> for String {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        let bytes = value
            .as_bytes()
            .ok_or_else(|| "cannot decode SQL Server NULL as String".to_owned())?;

        if matches!(value.type_info.kind(), MssqlType::VarChar) {
            return Ok(std::str::from_utf8(bytes)?.to_owned());
        }

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
    fn produces(&self) -> Option<MssqlTypeInfo> {
        Some(MssqlTypeInfo::with_size(
            MssqlType::VarBinary,
            varbinary_parameter_size(self.len()),
        ))
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        buf.extend_from_slice(self);
        Ok(IsNull::No)
    }
}

impl<'q> Encode<'q, Mssql> for &'q [u8] {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        <[u8] as Encode<Mssql>>::produces(*self)
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <[u8] as Encode<Mssql>>::encode_by_ref(*self, buf)
    }
}

impl Encode<'_, Mssql> for Vec<u8> {
    fn produces(&self) -> Option<MssqlTypeInfo> {
        <[u8] as Encode<Mssql>>::produces(self.as_slice())
    }

    fn encode_by_ref(&self, buf: &mut Vec<u8>) -> Result<IsNull, BoxDynError> {
        <[u8] as Encode<Mssql>>::encode_by_ref(self.as_slice(), buf)
    }
}

impl Decode<'_, Mssql> for Vec<u8> {
    fn decode(value: MssqlValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(<&[u8] as Decode<Mssql>>::decode(value)?.to_vec())
    }
}

impl<'r> Decode<'r, Mssql> for &'r [u8] {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        non_null_bytes(value, "bytes")
    }
}

fn nvarchar_parameter_size(value: &str) -> u16 {
    let bytes = value.encode_utf16().count().saturating_mul(2);
    if bytes > 8000 {
        u16::MAX
    } else {
        u16::try_from(std::cmp::max(2, bytes)).unwrap_or(u16::MAX)
    }
}

fn varbinary_parameter_size(len: usize) -> u16 {
    if len > 8000 {
        u16::MAX
    } else {
        u16::try_from(std::cmp::max(1, len)).unwrap_or(u16::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_scalars_use_lossless_parameter_types() {
        assert_eq!(MssqlTypeInfo::SMALLINT, <i8 as Type<Mssql>>::type_info());
        assert_eq!(MssqlTypeInfo::TINYINT, <u8 as Type<Mssql>>::type_info());
        assert_eq!(MssqlTypeInfo::INT, <u16 as Type<Mssql>>::type_info());
        assert_eq!(MssqlTypeInfo::BIGINT, <u32 as Type<Mssql>>::type_info());
    }

    #[test]
    fn encodes_unsigned_integer_scalars_without_saturation() {
        let mut buf = Vec::new();
        let _ = <u32 as Encode<Mssql>>::encode_by_ref(&u32::MAX, &mut buf).unwrap();
        assert_eq!(i64::from(u32::MAX).to_le_bytes(), buf.as_slice());

        buf.clear();
        let _ = <u16 as Encode<Mssql>>::encode_by_ref(&u16::MAX, &mut buf).unwrap();
        assert_eq!(i32::from(u16::MAX).to_le_bytes(), buf.as_slice());
    }

    #[test]
    fn decodes_integer_scalars_with_range_checks() {
        let value = MssqlValue::new(MssqlTypeInfo::INT, Some(65_535_i32.to_le_bytes().to_vec()));
        assert_eq!(
            65_535_u16,
            <u16 as Decode<Mssql>>::decode(value.as_ref()).unwrap()
        );

        let negative = MssqlValue::new(MssqlTypeInfo::INT, Some((-1_i32).to_le_bytes().to_vec()));
        assert!(<u16 as Decode<Mssql>>::decode(negative.as_ref()).is_err());

        let too_large = MssqlValue::new(MssqlTypeInfo::INT, Some(128_i32.to_le_bytes().to_vec()));
        assert!(<i8 as Decode<Mssql>>::decode(too_large.as_ref()).is_err());
    }

    #[test]
    fn decodes_borrowed_bytes() {
        let value = MssqlValue::new(MssqlTypeInfo::VARBINARY, Some(vec![1, 2, 3, 4]));
        let bytes = <&[u8] as Decode<Mssql>>::decode(value.as_ref()).unwrap();

        assert_eq!(&[1, 2, 3, 4], bytes);
    }

    #[test]
    fn decodes_ascii_varchar_as_utf8() {
        let value = MssqlValue::new(MssqlTypeInfo::VARCHAR, Some(b"hello".to_vec()));

        assert_eq!(
            "hello",
            <String as Decode<Mssql>>::decode(value.as_ref()).unwrap()
        );
    }

    #[test]
    fn decodes_nvarchar_as_utf16() {
        let mut data = Vec::new();
        for unit in "hello".encode_utf16() {
            data.extend_from_slice(&unit.to_le_bytes());
        }

        let value = MssqlValue::new(MssqlTypeInfo::NVARCHAR, Some(data));

        assert_eq!(
            "hello",
            <String as Decode<Mssql>>::decode(value.as_ref()).unwrap()
        );
    }
}
