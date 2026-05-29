use sqlx_core::Error;

use super::read::{read_b_varchar, read_u16_le, read_u32_le};
use super::type_info::TypeInfo;
use crate::{MssqlColumn, MssqlTypeInfo};

#[derive(Debug)]
pub(crate) struct ColMetaData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnData {
    #[allow(dead_code)]
    pub(crate) user_type: u32,
    pub(crate) flags: Flags,
    pub(crate) type_info: TypeInfo,
    pub(crate) col_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Flags(u16);

impl Flags {
    pub(crate) const fn from_bits_truncate(bits: u16) -> Self {
        Self(bits & 0x3f3f)
    }
}

impl ColMetaData {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Vec<MssqlColumn>, Error> {
        let count = read_u16_le(input)?;
        if count == 0xffff {
            return Ok(Vec::new());
        }

        let mut columns = Vec::with_capacity(usize::from(count));

        for ordinal in 0..usize::from(count) {
            let column = ColumnData::get(input)?;
            columns.push(MssqlColumn::new(
                ordinal,
                column.col_name,
                MssqlTypeInfo::from_protocol(&column.type_info),
            ));
        }

        Ok(columns)
    }
}

impl ColumnData {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, Error> {
        let user_type = read_u32_le(input)?;
        let flags = Flags::from_bits_truncate(read_u16_le(input)?);
        let type_info = TypeInfo::get(input).map_err(|error| Error::Protocol(error.to_string()))?;
        let col_name = read_b_varchar(input)?;

        Ok(Self {
            user_type,
            flags,
            type_info,
            col_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx_core::column::Column;

    #[test]
    fn parses_column_metadata() {
        let mut input = &[
            1,
            0, // count
            0,
            0,
            0,
            0, // user type
            0,
            0, // flags
            super::super::type_info::DataType::Int as u8,
            6,
            b'a',
            0,
            b'n',
            0,
            b's',
            0,
            b'w',
            0,
            b'e',
            0,
            b'r',
            0,
        ][..];

        let columns = ColMetaData::get(&mut input).unwrap();

        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].name(), "answer");
    }
}
