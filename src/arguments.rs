use std::fmt::{self, Write};

use sqlx_core::arguments::Arguments;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;

use crate::{Mssql, MssqlType, MssqlTypeInfo};

const DATA_TYPE_INTN: u8 = 0x26;
const DATA_TYPE_BITN: u8 = 0x68;
const DATA_TYPE_FLOATN: u8 = 0x6d;
const DATA_TYPE_BIGVARBINARY: u8 = 0xa5;
const DATA_TYPE_NVARCHAR: u8 = 0xe7;
const DEFAULT_COLLATION: [u8; 5] = [0x81, 0x04, 0xd0, 0x00, 0x34];
const STATUS_BY_REF_VALUE: u8 = 0x01;

/// SQL Server argument buffer.
#[derive(Debug, Default, Clone)]
pub struct MssqlArguments {
    len: usize,
    data: Vec<u8>,
    declarations: String,
}

impl MssqlArguments {
    /// Returns `true` when no arguments were added.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn declarations(&self) -> &str {
        &self.declarations
    }

    fn add_parameter(
        &mut self,
        type_info: MssqlTypeInfo,
        encoded: Vec<u8>,
        is_null: bool,
    ) -> Result<(), BoxDynError> {
        self.len += 1;
        let name = format!("@p{}", self.len);

        if !self.declarations.is_empty() {
            self.declarations.push(',');
        }

        write!(
            self.declarations,
            "{name} {}",
            declaration(&type_info, encoded.len(), is_null)?
        )?;

        write_parameter(&mut self.data, &name, &type_info, &encoded, is_null)?;
        Ok(())
    }
}

impl Arguments for MssqlArguments {
    type Database = Mssql;

    fn reserve(&mut self, _additional: usize, _size: usize) {}

    fn add<'t, T>(&mut self, value: T) -> Result<(), BoxDynError>
    where
        T: Encode<'t, Self::Database> + Type<Self::Database>,
    {
        let type_info = value.produces().unwrap_or_else(T::type_info);
        let mut encoded = Vec::with_capacity(value.size_hint());
        let is_null = matches!(value.encode(&mut encoded)?, IsNull::Yes);
        self.add_parameter(type_info, encoded, is_null)?;
        Ok(())
    }

    fn len(&self) -> usize {
        self.len
    }

    fn format_placeholder<W: Write>(&self, writer: &mut W) -> fmt::Result {
        write!(writer, "@p{}", self.len)
    }
}

pub(crate) fn write_parameter(
    out: &mut Vec<u8>,
    name: &str,
    type_info: &MssqlTypeInfo,
    encoded: &[u8],
    is_null: bool,
) -> Result<(), BoxDynError> {
    write_parameter_with_status(out, name, 0, type_info, encoded, is_null)
}

pub(crate) fn write_output_i32_parameter(
    out: &mut Vec<u8>,
    name: &str,
    value: i32,
) -> Result<(), BoxDynError> {
    write_parameter_with_status(
        out,
        name,
        STATUS_BY_REF_VALUE,
        &MssqlTypeInfo::INT,
        &value.to_le_bytes(),
        false,
    )
}

fn write_parameter_with_status(
    out: &mut Vec<u8>,
    name: &str,
    status: u8,
    type_info: &MssqlTypeInfo,
    encoded: &[u8],
    is_null: bool,
) -> Result<(), BoxDynError> {
    write_b_varchar(out, name)?;
    out.push(status);
    write_type_info(out, type_info, encoded.len(), is_null)?;
    write_param_len_data(out, type_info, encoded, is_null)?;
    Ok(())
}

pub(crate) fn write_nvarchar_parameter(
    out: &mut Vec<u8>,
    name: &str,
    value: &str,
) -> Result<(), BoxDynError> {
    let mut encoded = Vec::with_capacity(value.len() * 2);
    write_utf16(&mut encoded, value);
    write_parameter(out, name, &MssqlTypeInfo::NVARCHAR, &encoded, false)
}

pub(crate) fn write_null_nvarchar_parameter(
    out: &mut Vec<u8>,
    name: &str,
) -> Result<(), BoxDynError> {
    write_parameter(out, name, &MssqlTypeInfo::NVARCHAR, &[], true)
}

pub(crate) fn type_declaration(type_info: &MssqlTypeInfo) -> Result<&'static str, BoxDynError> {
    Ok(match type_info.kind() {
        MssqlType::Bit => "bit",
        MssqlType::TinyInt => "tinyint",
        MssqlType::SmallInt => "smallint",
        MssqlType::Int => "int",
        MssqlType::BigInt => "bigint",
        MssqlType::Real => "real",
        MssqlType::Float => "float",
        MssqlType::NVarChar => "nvarchar(max)",
        MssqlType::VarBinary => "varbinary(max)",
        other => return Err(format!("SQL Server arguments do not support type {other:?}").into()),
    })
}

fn write_type_info(
    out: &mut Vec<u8>,
    type_info: &MssqlTypeInfo,
    encoded_len: usize,
    is_null: bool,
) -> Result<(), BoxDynError> {
    match type_info.kind() {
        MssqlType::Bit => {
            out.push(DATA_TYPE_BITN);
            out.push(1);
        }
        MssqlType::TinyInt => {
            out.push(DATA_TYPE_INTN);
            out.push(1);
        }
        MssqlType::SmallInt => {
            out.push(DATA_TYPE_INTN);
            out.push(2);
        }
        MssqlType::Int => {
            out.push(DATA_TYPE_INTN);
            out.push(4);
        }
        MssqlType::BigInt => {
            out.push(DATA_TYPE_INTN);
            out.push(8);
        }
        MssqlType::Real => {
            out.push(DATA_TYPE_FLOATN);
            out.push(4);
        }
        MssqlType::Float => {
            out.push(DATA_TYPE_FLOATN);
            out.push(8);
        }
        MssqlType::NVarChar => {
            out.push(DATA_TYPE_NVARCHAR);
            out.extend_from_slice(&nvarchar_type_size(encoded_len, is_null)?.to_le_bytes());
            out.extend_from_slice(&DEFAULT_COLLATION);
        }
        MssqlType::VarBinary => {
            out.push(DATA_TYPE_BIGVARBINARY);
            out.extend_from_slice(&bounded_short_len(encoded_len, is_null)?.to_le_bytes());
        }
        other => return Err(format!("SQL Server arguments do not support type {other:?}").into()),
    }

    Ok(())
}

fn write_param_len_data(
    out: &mut Vec<u8>,
    type_info: &MssqlTypeInfo,
    encoded: &[u8],
    is_null: bool,
) -> Result<(), BoxDynError> {
    match type_info.kind() {
        MssqlType::Bit
        | MssqlType::TinyInt
        | MssqlType::SmallInt
        | MssqlType::Int
        | MssqlType::BigInt
        | MssqlType::Real
        | MssqlType::Float => {
            out.push(if is_null {
                0
            } else {
                u8::try_from(encoded.len())?
            });
        }
        MssqlType::NVarChar | MssqlType::VarBinary => {
            let len = if is_null {
                u16::MAX
            } else {
                u16::try_from(encoded.len())?
            };
            out.extend_from_slice(&len.to_le_bytes());
        }
        other => return Err(format!("SQL Server arguments do not support type {other:?}").into()),
    }

    if !is_null {
        out.extend_from_slice(encoded);
    }

    Ok(())
}

fn declaration(
    type_info: &MssqlTypeInfo,
    encoded_len: usize,
    is_null: bool,
) -> Result<String, BoxDynError> {
    Ok(match type_info.kind() {
        MssqlType::Bit => "bit".to_owned(),
        MssqlType::TinyInt => "tinyint".to_owned(),
        MssqlType::SmallInt => "smallint".to_owned(),
        MssqlType::Int => "int".to_owned(),
        MssqlType::BigInt => "bigint".to_owned(),
        MssqlType::Real => "real".to_owned(),
        MssqlType::Float => "float".to_owned(),
        MssqlType::NVarChar => format!("nvarchar({})", nvarchar_chars(encoded_len, is_null)?),
        MssqlType::VarBinary => format!("varbinary({})", bounded_short_len(encoded_len, is_null)?),
        other => return Err(format!("SQL Server arguments do not support type {other:?}").into()),
    })
}

fn nvarchar_chars(encoded_len: usize, is_null: bool) -> Result<u16, BoxDynError> {
    Ok(nvarchar_type_size(encoded_len, is_null)? / 2)
}

fn nvarchar_type_size(encoded_len: usize, is_null: bool) -> Result<u16, BoxDynError> {
    let len = if is_null {
        2
    } else {
        std::cmp::max(2, encoded_len)
    };
    Ok(u16::try_from(len)?)
}

fn bounded_short_len(encoded_len: usize, is_null: bool) -> Result<u16, BoxDynError> {
    let len = if is_null {
        1
    } else {
        std::cmp::max(1, encoded_len)
    };
    Ok(u16::try_from(len)?)
}

fn write_b_varchar(out: &mut Vec<u8>, value: &str) -> Result<(), BoxDynError> {
    let char_len = value.encode_utf16().count();
    out.push(u8::try_from(char_len)?);
    write_utf16(out, value);
    Ok(())
}

fn write_utf16(out: &mut Vec<u8>, value: &str) {
    for unit in value.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sql_server_style_placeholders() {
        let args = MssqlArguments {
            len: 3,
            data: Vec::new(),
            declarations: String::new(),
        };
        let mut out = String::new();

        args.format_placeholder(&mut out).unwrap();

        assert_eq!("@p3", out);
    }

    #[test]
    fn records_declarations_and_rpc_argument_data() {
        let mut args = MssqlArguments::default();

        args.add(7_i32).unwrap();
        args.add("hi").unwrap();

        assert_eq!("@p1 int,@p2 nvarchar(2)", args.declarations());
        assert!(args
            .data()
            .windows(2)
            .any(|bytes| bytes == [DATA_TYPE_INTN, 4]));
        assert!(args
            .data()
            .windows(8)
            .any(|bytes| bytes == [DATA_TYPE_NVARCHAR, 4, 0, 0x81, 0x04, 0xd0, 0x00, 0x34]));
    }

    #[test]
    fn declares_lossless_integer_parameter_types() {
        let mut args = MssqlArguments::default();

        args.add(-5_i8).unwrap();
        args.add(255_u8).unwrap();
        args.add(65_535_u16).unwrap();
        args.add(u32::MAX).unwrap();

        assert_eq!(
            "@p1 smallint,@p2 tinyint,@p3 int,@p4 bigint",
            args.declarations()
        );
        assert!(args
            .data()
            .windows(2)
            .any(|bytes| bytes == [DATA_TYPE_INTN, 1]));
        assert!(args
            .data()
            .windows(2)
            .any(|bytes| bytes == [DATA_TYPE_INTN, 8]));
    }
}
