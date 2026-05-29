use sqlx_core::Error;

use super::col_meta_data::Flags;
use super::read::{read_b_varchar, read_u16_le, read_u32_le, read_u8};
use super::type_info::TypeInfo;
use crate::{MssqlTypeInfo, MssqlValue};

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ReturnValue {
    param_ordinal: u16,
    param_name: String,
    status: ReturnValueStatus,
    user_type: u32,
    flags: Flags,
    pub(crate) type_info: TypeInfo,
    pub(crate) value: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReturnValueStatus(u8);

impl ReturnValueStatus {
    pub(crate) const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & 0x03)
    }
}

impl ReturnValue {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, Error> {
        let param_ordinal = read_u16_le(input)?;
        let param_name = read_b_varchar(input)?;
        let status = ReturnValueStatus::from_bits_truncate(read_u8(input)?);
        let user_type = read_u32_le(input)?;
        let flags = Flags::from_bits_truncate(read_u16_le(input)?);
        let type_info = TypeInfo::get(input).map_err(|error| Error::Protocol(error.to_string()))?;
        let value = type_info
            .get_value(input)
            .map_err(|error| Error::Protocol(error.to_string()))?;

        Ok(Self {
            param_ordinal,
            param_name,
            status,
            user_type,
            flags,
            type_info,
            value,
        })
    }

    pub(crate) fn into_value(self) -> MssqlValue {
        MssqlValue::new(MssqlTypeInfo::from_protocol(&self.type_info), self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::type_info::DataType;

    #[test]
    fn parses_return_value() {
        let mut input = &[
            0,
            0, // ordinal
            0, // name
            1, // status
            0,
            0,
            0,
            0, // user type
            0,
            0, // flags
            DataType::IntN as u8,
            4,
            4,
            1,
            0,
            0,
            0,
        ][..];

        let return_value = ReturnValue::get(&mut input).unwrap();

        assert_eq!(return_value.value, Some(vec![1, 0, 0, 0]));
    }
}
