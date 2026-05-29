use std::fmt::Write;

use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;

use crate::Mssql;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CollationFlags(u8);

impl CollationFlags {
    pub(crate) const IGNORE_CASE: Self = Self(1 << 0);
    pub(crate) const IGNORE_ACCENT: Self = Self(1 << 1);
    pub(crate) const IGNORE_WIDTH: Self = Self(1 << 2);
    pub(crate) const IGNORE_KANA: Self = Self(1 << 3);
    pub(crate) const BINARY: Self = Self(1 << 4);
    pub(crate) const BINARY2: Self = Self(1 << 5);

    pub(crate) const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & 0x3f)
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) struct Collation {
    pub(crate) locale: u32,
    pub(crate) flags: CollationFlags,
    pub(crate) sort: u8,
    pub(crate) version: u8,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
#[repr(u8)]
pub(crate) enum DataType {
    // fixed-length data types
    // https://docs.microsoft.com/en-us/openspecs/sql_server_protocols/ms-sstds/d33ef17b-7e53-4380-ad11-2ba42c8dda8d
    Null = 0x1f,
    TinyInt = 0x30,
    Bit = 0x32,
    SmallInt = 0x34,
    Int = 0x38,
    SmallDateTime = 0x3a,
    Real = 0x3b,
    Money = 0x3c,
    DateTime = 0x3d,
    Float = 0x3e,
    SmallMoney = 0x7a,
    BigInt = 0x7f,

    // variable-length data types
    // https://docs.microsoft.com/en-us/openspecs/windows_protocols/ms-tds/ce3183a6-9d89-47e8-a02f-de5a1a1303de

    // byte length
    Guid = 0x24,
    IntN = 0x26,
    Decimal = 0x37,
    Numeric = 0x3f,
    BitN = 0x68,
    DecimalN = 0x6a,
    NumericN = 0x6c,
    FloatN = 0x6d,
    MoneyN = 0x6e,
    DateTimeN = 0x6f,
    DateN = 0x28,
    TimeN = 0x29,
    DateTime2N = 0x2a,
    DateTimeOffsetN = 0x2b,
    Char = 0x2f,
    VarChar = 0x27,
    Binary = 0x2d,
    VarBinary = 0x25,

    // short length
    BigVarBinary = 0xa5,
    BigVarChar = 0xa7,
    BigBinary = 0xad,
    BigChar = 0xaf,
    NVarChar = 0xe7,
    NChar = 0xef,
    Xml = 0xf1,
    UserDefined = 0xf0,

    // long length
    Text = 0x23,
    Image = 0x22,
    NText = 0x63,
    Variant = 0x62,
}

// http://msdn.microsoft.com/en-us/library/dd358284.aspx
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TypeInfo {
    pub(crate) ty: DataType,
    pub(crate) size: u32,
    pub(crate) scale: u8,
    pub(crate) precision: u8,
    pub(crate) collation: Option<Collation>,
}

impl TypeInfo {
    pub(crate) const fn new(ty: DataType, size: u32) -> Self {
        Self {
            ty,
            size,
            scale: 0,
            precision: 0,
            collation: None,
        }
    }

    // reads a TYPE_INFO from the buffer
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, TypeInfoError> {
        let ty = DataType::get(input)?;

        Ok(match ty {
            DataType::Null => Self::new(ty, 0),

            DataType::TinyInt | DataType::Bit => Self::new(ty, 1),

            DataType::SmallInt => Self::new(ty, 2),

            DataType::Int | DataType::SmallDateTime | DataType::Real | DataType::SmallMoney => {
                Self::new(ty, 4)
            }

            DataType::BigInt | DataType::Money | DataType::DateTime | DataType::Float => {
                Self::new(ty, 8)
            }

            DataType::DateN => Self::new(ty, 3),

            DataType::TimeN | DataType::DateTime2N | DataType::DateTimeOffsetN => {
                let scale = read_u8(input)?;

                let mut size = match scale {
                    0..=2 => 3,
                    3..=4 => 4,
                    5..=7 => 5,
                    scale => {
                        return Err(TypeInfoError::InvalidScale { ty, scale });
                    }
                };

                match ty {
                    DataType::DateTime2N => {
                        size += 3;
                    }
                    DataType::DateTimeOffsetN => {
                        size += 5;
                    }
                    _ => {}
                }

                Self {
                    scale,
                    size,
                    ty,
                    precision: 0,
                    collation: None,
                }
            }

            DataType::Guid
            | DataType::IntN
            | DataType::BitN
            | DataType::FloatN
            | DataType::MoneyN
            | DataType::DateTimeN
            | DataType::Char
            | DataType::VarChar
            | DataType::Binary
            | DataType::VarBinary => Self::new(ty, u32::from(read_u8(input)?)),

            DataType::Decimal | DataType::Numeric | DataType::DecimalN | DataType::NumericN => {
                let size = u32::from(read_u8(input)?);
                let precision = read_u8(input)?;
                let scale = read_u8(input)?;

                Self {
                    size,
                    precision,
                    scale,
                    ty,
                    collation: None,
                }
            }

            DataType::BigVarBinary | DataType::BigBinary => {
                Self::new(ty, u32::from(read_u16_le(input)?))
            }

            DataType::BigVarChar | DataType::BigChar | DataType::NVarChar | DataType::NChar => {
                let size = u32::from(read_u16_le(input)?);
                let collation = Collation::get(input)?;

                Self {
                    ty,
                    size,
                    collation: Some(collation),
                    scale: 0,
                    precision: 0,
                }
            }

            DataType::Xml
            | DataType::UserDefined
            | DataType::Text
            | DataType::Image
            | DataType::NText
            | DataType::Variant => {
                return Err(TypeInfoError::UnsupportedDataType(ty));
            }
        })
    }

    // writes a TYPE_INFO to the buffer
    pub(crate) fn put(&self, out: &mut Vec<u8>) -> Result<(), BoxDynError> {
        out.push(self.ty as u8);

        match self.ty {
            DataType::Null
            | DataType::TinyInt
            | DataType::Bit
            | DataType::SmallInt
            | DataType::Int
            | DataType::SmallDateTime
            | DataType::Real
            | DataType::SmallMoney
            | DataType::BigInt
            | DataType::Money
            | DataType::DateTime
            | DataType::Float => {}

            DataType::TimeN | DataType::DateTime2N | DataType::DateTimeOffsetN => {
                out.push(self.scale);
            }

            DataType::Guid
            | DataType::IntN
            | DataType::BitN
            | DataType::FloatN
            | DataType::MoneyN
            | DataType::DateTimeN
            | DataType::DateN
            | DataType::Char
            | DataType::VarChar
            | DataType::Binary
            | DataType::VarBinary => {
                out.push(u8::try_from(self.size)?);
            }

            DataType::Decimal | DataType::Numeric | DataType::DecimalN | DataType::NumericN => {
                out.push(u8::try_from(self.size)?);
                out.push(self.precision);
                out.push(self.scale);
            }

            DataType::BigVarBinary | DataType::BigBinary => {
                out.extend_from_slice(&u16::try_from(self.size)?.to_le_bytes());
            }

            DataType::BigVarChar | DataType::BigChar | DataType::NVarChar | DataType::NChar => {
                out.extend_from_slice(&u16::try_from(self.size)?.to_le_bytes());

                if let Some(collation) = &self.collation {
                    collation.put(out);
                } else {
                    out.extend_from_slice(&0_u32.to_le_bytes());
                    out.push(0);
                }
            }

            DataType::Xml
            | DataType::UserDefined
            | DataType::Text
            | DataType::Image
            | DataType::NText
            | DataType::Variant => {
                log::error!("Unsupported mssql data type argument writing {:?}", self.ty);
            }
        }

        Ok(())
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self.ty, DataType::Null)
    }

    pub(crate) fn get_value(&self, input: &mut &[u8]) -> Result<Option<Vec<u8>>, TypeInfoError> {
        Ok(match self.ty {
            DataType::Null
            | DataType::TinyInt
            | DataType::Bit
            | DataType::SmallInt
            | DataType::Int
            | DataType::SmallDateTime
            | DataType::Real
            | DataType::Money
            | DataType::DateTime
            | DataType::Float
            | DataType::SmallMoney
            | DataType::BigInt => Some(take(input, self.size as usize)?.to_vec()),

            DataType::Guid
            | DataType::IntN
            | DataType::Decimal
            | DataType::Numeric
            | DataType::BitN
            | DataType::DecimalN
            | DataType::NumericN
            | DataType::FloatN
            | DataType::MoneyN
            | DataType::DateN
            | DataType::DateTimeN
            | DataType::TimeN
            | DataType::DateTime2N
            | DataType::DateTimeOffsetN => {
                let size = read_u8(input)?;

                if size == 0 || size == 0xff {
                    None
                } else {
                    Some(take(input, usize::from(size))?.to_vec())
                }
            }

            DataType::Char | DataType::VarChar | DataType::Binary | DataType::VarBinary => {
                let size = read_u8(input)?;
                if size == 0xff {
                    None
                } else {
                    Some(take(input, usize::from(size))?.to_vec())
                }
            }

            DataType::BigVarBinary
            | DataType::BigVarChar
            | DataType::BigBinary
            | DataType::BigChar
            | DataType::NVarChar
            | DataType::NChar
            | DataType::Xml
            | DataType::UserDefined => {
                if self.size == 0xffff {
                    self.get_big_blob(input)?
                } else {
                    let size = read_u16_le(input)?;
                    if size == 0xffff {
                        None
                    } else {
                        Some(take(input, usize::from(size))?.to_vec())
                    }
                }
            }

            DataType::Text | DataType::Image | DataType::NText | DataType::Variant => {
                let size = read_u32_le(input)?;

                if size == 0xffff_ffff {
                    None
                } else {
                    Some(take(input, usize::try_from(size).unwrap())?.to_vec())
                }
            }
        })
    }

    pub(crate) fn get_big_blob(&self, input: &mut &[u8]) -> Result<Option<Vec<u8>>, TypeInfoError> {
        // Unknown size, length-prefixed blobs.
        let len = read_u64_le(input)?;

        let mut data = match len {
            // NULL
            0xffff_ffff_ffff_ffff => return Ok(None),
            // Unknown size
            0xffff_ffff_ffff_fffe => Vec::new(),
            // Known size
            _ => Vec::with_capacity(usize::try_from(len).unwrap()),
        };

        loop {
            let chunk_size = read_u32_le(input)? as usize;

            if chunk_size == 0 {
                break;
            }

            data.extend_from_slice(take(input, chunk_size)?);
        }

        Ok(Some(data))
    }

    pub(crate) fn put_value<'q, T: Encode<'q, Mssql>>(
        &self,
        out: &mut Vec<u8>,
        value: T,
    ) -> Result<(), BoxDynError> {
        match self.ty {
            DataType::Null
            | DataType::TinyInt
            | DataType::Bit
            | DataType::SmallInt
            | DataType::Int
            | DataType::SmallDateTime
            | DataType::Real
            | DataType::Money
            | DataType::DateTime
            | DataType::DateN
            | DataType::Float
            | DataType::SmallMoney
            | DataType::BigInt => {
                self.put_fixed_value(out, value)?;
            }

            DataType::Guid
            | DataType::IntN
            | DataType::Decimal
            | DataType::Numeric
            | DataType::BitN
            | DataType::DecimalN
            | DataType::NumericN
            | DataType::FloatN
            | DataType::MoneyN
            | DataType::DateTimeN
            | DataType::TimeN
            | DataType::DateTime2N
            | DataType::DateTimeOffsetN
            | DataType::Char
            | DataType::VarChar
            | DataType::Binary
            | DataType::VarBinary => {
                self.put_byte_len_value(out, value)?;
            }

            DataType::BigVarBinary
            | DataType::BigVarChar
            | DataType::BigBinary
            | DataType::BigChar
            | DataType::NVarChar
            | DataType::NChar
            | DataType::Xml
            | DataType::UserDefined => {
                if self.size == 0xffff {
                    self.put_big_blob(out, value)?;
                } else {
                    self.put_short_len_value(out, value)?;
                }
            }

            DataType::Text | DataType::Image | DataType::NText | DataType::Variant => {
                self.put_long_len_value(out, value)?;
            }
        }

        Ok(())
    }

    pub(crate) fn put_fixed_value<'q, T: Encode<'q, Mssql>>(
        &self,
        out: &mut Vec<u8>,
        value: T,
    ) -> Result<(), BoxDynError> {
        let _ = value.encode(out)?;
        Ok(())
    }

    pub(crate) fn put_byte_len_value<'q, T: Encode<'q, Mssql>>(
        &self,
        out: &mut Vec<u8>,
        value: T,
    ) -> Result<(), BoxDynError> {
        let offset = out.len();
        out.push(0);

        let size = if let IsNull::Yes = value.encode(out)? {
            0xff
        } else {
            u8::try_from(out.len() - offset - 1)?
        };

        out[offset] = size;
        Ok(())
    }

    pub(crate) fn put_short_len_value<'q, T: Encode<'q, Mssql>>(
        &self,
        out: &mut Vec<u8>,
        value: T,
    ) -> Result<(), BoxDynError> {
        let offset = out.len();
        out.extend_from_slice(&0_u16.to_le_bytes());

        let size = if let IsNull::Yes = value.encode(out)? {
            0xffff
        } else {
            u16::try_from(out.len() - offset - 2)?
        };

        out[offset..(offset + 2)].copy_from_slice(&size.to_le_bytes());
        Ok(())
    }

    pub(crate) fn put_big_blob<'q, T: Encode<'q, Mssql>>(
        &self,
        out: &mut Vec<u8>,
        value: T,
    ) -> Result<(), BoxDynError> {
        // Multiple chunks are not supported yet.
        let start_of_value = out.len();
        out.extend_from_slice(&0_u64.to_le_bytes());
        let start_of_chunk = out.len();
        out.extend_from_slice(&0_u32.to_le_bytes());
        let start_of_bytes = out.len();

        let size = if let IsNull::Yes = value.encode(out)? {
            unreachable!("put_big_blob should never be called with NULL value");
        } else {
            u32::try_from(out.len() - start_of_bytes).expect("blobs >4GB not supported")
        };

        out[start_of_value..(start_of_value + 4)].copy_from_slice(&size.to_le_bytes());
        out[start_of_chunk..(start_of_chunk + 4)].copy_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        Ok(())
    }

    pub(crate) fn put_long_len_value<'q, T: Encode<'q, Mssql>>(
        &self,
        out: &mut Vec<u8>,
        value: T,
    ) -> Result<(), BoxDynError> {
        let offset = out.len();
        out.extend_from_slice(&0_u32.to_le_bytes());

        let size = if let IsNull::Yes = value.encode(out)? {
            0xffff_ffff
        } else {
            u32::try_from(out.len() - offset - 4)?
        };

        out[offset..(offset + 4)].copy_from_slice(&size.to_le_bytes());
        Ok(())
    }

    pub(crate) fn name(&self) -> &'static str {
        match self.ty {
            DataType::Null => "NULL",
            DataType::TinyInt => "TINYINT",
            DataType::SmallInt => "SMALLINT",
            DataType::Int => "INT",
            DataType::BigInt => "BIGINT",
            DataType::Real => "REAL",
            DataType::Float => "FLOAT",

            DataType::IntN => match self.size {
                1 => "TINYINT",
                2 => "SMALLINT",
                4 => "INT",
                8 => "BIGINT",
                n => unreachable!("invalid size {} for int", n),
            },

            DataType::FloatN => match self.size {
                4 => "REAL",
                8 => "FLOAT",
                n => unreachable!("invalid size {} for float", n),
            },

            DataType::VarChar => "VARCHAR",
            DataType::NVarChar => "NVARCHAR",
            DataType::BigVarChar => "BIGVARCHAR",
            DataType::Char => "CHAR",
            DataType::BigChar => "BIGCHAR",
            DataType::NChar => "NCHAR",
            DataType::VarBinary => "VARBINARY",
            DataType::BigVarBinary => "BIGVARBINARY",
            DataType::Binary => "BINARY",
            DataType::BigBinary => "BIGBINARY",
            DataType::DateN => "DATE",
            DataType::DateTimeN => "DATETIME",
            DataType::DateTime2N => "DATETIME2",
            DataType::DateTimeOffsetN => "DATETIMEOFFSET",

            DataType::Bit => "BIT",
            DataType::SmallDateTime => "SMALLDATETIME",
            DataType::Money => "MONEY",
            DataType::DateTime => "DATETIME",
            DataType::SmallMoney => "SMALLMONEY",
            DataType::Guid => "UNIQUEIDENTIFIER",
            DataType::Decimal => "DECIMAL",
            DataType::Numeric => "NUMERIC",
            DataType::BitN => "BIT",
            DataType::DecimalN => "DECIMAL",
            DataType::NumericN => "NUMERIC",
            DataType::MoneyN => "MONEY",
            DataType::TimeN => "TIME",
            DataType::Xml => "XML",
            DataType::UserDefined => "USER_DEFINED_TYPE",
            DataType::Text => "TEXT",
            DataType::Image => "IMAGE",
            DataType::NText => "NTEXT",
            DataType::Variant => "SQL_VARIANT",
        }
    }

    pub(crate) fn fmt(&self, out: &mut String) {
        match self.ty {
            DataType::Null => out.push_str("nvarchar(1)"),
            DataType::TinyInt => out.push_str("tinyint"),
            DataType::SmallInt => out.push_str("smallint"),
            DataType::Int => out.push_str("int"),
            DataType::BigInt => out.push_str("bigint"),
            DataType::Real => out.push_str("real"),
            DataType::Float => out.push_str("float"),
            DataType::Bit => out.push_str("bit"),

            DataType::IntN => out.push_str(match self.size {
                1 => "tinyint",
                2 => "smallint",
                4 => "int",
                8 => "bigint",
                n => unreachable!("invalid size {} for int", n),
            }),

            DataType::FloatN => out.push_str(match self.size {
                4 => "real",
                8 => "float",
                n => unreachable!("invalid size {} for float", n),
            }),

            DataType::NVarChar | DataType::NChar => {
                out.push_str(match self.ty {
                    DataType::NVarChar => "nvarchar",
                    DataType::NChar => "nchar",
                    _ => unreachable!(),
                });

                if self.size == 0xffff {
                    out.push_str("(max)");
                } else {
                    let _ = write!(out, "({})", self.size / 2);
                }
            }

            DataType::VarChar
            | DataType::BigVarChar
            | DataType::Char
            | DataType::BigChar
            | DataType::VarBinary
            | DataType::BigVarBinary
            | DataType::Binary
            | DataType::BigBinary => {
                out.push_str(match self.ty {
                    DataType::VarChar => "varchar",
                    DataType::BigVarChar => "bigvarchar",
                    DataType::Char => "char",
                    DataType::BigChar => "bigchar",
                    DataType::VarBinary => "varbinary",
                    DataType::BigVarBinary => "varbinary",
                    DataType::Binary => "binary",
                    DataType::BigBinary => "binary",
                    _ => unreachable!(),
                });

                if self.size == 0xffff {
                    out.push_str("(max)");
                } else {
                    let _ = write!(out, "({})", self.size);
                }
            }

            DataType::BitN => {
                out.push_str("bit");
            }

            DataType::DateN => {
                out.push_str("date");
            }

            DataType::DateTime | DataType::DateTimeN => {
                out.push_str("datetime");
            }

            DataType::DateTime2N => {
                let _ = write!(out, "datetime2({})", self.scale);
            }

            DataType::DateTimeOffsetN => {
                let _ = write!(out, "datetimeoffset({})", self.scale);
            }

            DataType::TimeN => {
                let _ = write!(out, "time({})", self.scale);
            }
            DataType::SmallDateTime => out.push_str("smalldatetime"),
            DataType::Money => out.push_str("money"),
            DataType::SmallMoney => out.push_str("smallmoney"),
            DataType::Guid => out.push_str("uniqueidentifier"),
            DataType::Decimal => out.push_str("decimal"),
            DataType::Numeric => out.push_str("numeric"),
            DataType::DecimalN => {
                let _ = write!(out, "decimal({},{})", self.precision, self.scale);
            }
            DataType::NumericN => {
                let _ = write!(out, "numeric({},{})", self.precision, self.scale);
            }
            DataType::MoneyN => {
                let _ = write!(out, "money({})", self.scale);
            }
            DataType::Xml => out.push_str("xml"),
            DataType::UserDefined => out.push_str("user_defined_type"),
            DataType::Text => out.push_str("text"),
            DataType::Image => out.push_str("image"),
            DataType::NText => out.push_str("ntext"),
            DataType::Variant => out.push_str("sql_variant"),
        }
    }
}

impl DataType {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, TypeInfoError> {
        Ok(match read_u8(input)? {
            0x1f => DataType::Null,
            0x30 => DataType::TinyInt,
            0x32 => DataType::Bit,
            0x34 => DataType::SmallInt,
            0x38 => DataType::Int,
            0x3a => DataType::SmallDateTime,
            0x3b => DataType::Real,
            0x3c => DataType::Money,
            0x3d => DataType::DateTime,
            0x3e => DataType::Float,
            0x7a => DataType::SmallMoney,
            0x7f => DataType::BigInt,
            0x24 => DataType::Guid,
            0x26 => DataType::IntN,
            0x37 => DataType::Decimal,
            0x3f => DataType::Numeric,
            0x68 => DataType::BitN,
            0x6a => DataType::DecimalN,
            0x6c => DataType::NumericN,
            0x6d => DataType::FloatN,
            0x6e => DataType::MoneyN,
            0x6f => DataType::DateTimeN,
            0x28 => DataType::DateN,
            0x29 => DataType::TimeN,
            0x2a => DataType::DateTime2N,
            0x2b => DataType::DateTimeOffsetN,
            0x2f => DataType::Char,
            0x27 => DataType::VarChar,
            0x2d => DataType::Binary,
            0x25 => DataType::VarBinary,
            0xa5 => DataType::BigVarBinary,
            0xa7 => DataType::BigVarChar,
            0xad => DataType::BigBinary,
            0xaf => DataType::BigChar,
            0xe7 => DataType::NVarChar,
            0xef => DataType::NChar,
            0xf1 => DataType::Xml,
            0xf0 => DataType::UserDefined,
            0x23 => DataType::Text,
            0x22 => DataType::Image,
            0x63 => DataType::NText,
            0x62 => DataType::Variant,
            ty => return Err(TypeInfoError::UnknownDataType(ty)),
        })
    }
}

impl Collation {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Collation, TypeInfoError> {
        let locale_sort_version = read_u32_le(input)?;
        let locale = locale_sort_version & 0xfffff;
        let flags = CollationFlags::from_bits_truncate(((locale_sort_version >> 20) & 0xff) as u8);
        let version = (locale_sort_version >> 28) as u8;
        let sort = read_u8(input)?;

        Ok(Collation {
            locale,
            flags,
            sort,
            version,
        })
    }

    pub(crate) fn put(&self, out: &mut Vec<u8>) {
        let locale_sort_version =
            self.locale | ((u32::from(self.flags.bits())) << 20) | ((self.version as u32) << 28);

        out.extend_from_slice(&locale_sort_version.to_le_bytes());
        out.push(self.sort);
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum TypeInfoError {
    #[error("TDS TYPE_INFO ended unexpectedly")]
    UnexpectedEof,
    #[error("unknown TDS data type 0x{0:02x}")]
    UnknownDataType(u8),
    #[error("unsupported TDS data type {0:?}")]
    UnsupportedDataType(DataType),
    #[error("invalid scale {scale} for type {ty:?}")]
    InvalidScale { ty: DataType, scale: u8 },
}

fn read_u8(input: &mut &[u8]) -> Result<u8, TypeInfoError> {
    let bytes = take(input, 1)?;
    Ok(bytes[0])
}

fn read_u16_le(input: &mut &[u8]) -> Result<u16, TypeInfoError> {
    let bytes = take(input, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(input: &mut &[u8]) -> Result<u32, TypeInfoError> {
    let bytes = take(input, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(input: &mut &[u8]) -> Result<u64, TypeInfoError> {
    let bytes = take(input, 8)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], TypeInfoError> {
    let bytes = input.get(..len).ok_or(TypeInfoError::UnexpectedEof)?;
    *input = &input[len..];
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_type_info_and_value_with_old_wire_constants() {
        let mut input = &[
            DataType::IntN as u8,
            4,
            4,
            1,
            0,
            0,
            0,
            0xfe,
            0,
            0,
            0xe0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ][..];

        let type_info = TypeInfo::get(&mut input).unwrap();

        assert_eq!(type_info, TypeInfo::new(DataType::IntN, 4));
        assert_eq!(
            Some(vec![1, 0, 0, 0]),
            type_info.get_value(&mut input).unwrap()
        );
    }

    #[test]
    fn put_round_trips_collation_for_nvarchar_type_info() {
        let type_info = TypeInfo {
            ty: DataType::NVarChar,
            size: 8,
            scale: 0,
            precision: 0,
            collation: Some(Collation {
                locale: 0x0409,
                flags: CollationFlags::IGNORE_CASE,
                sort: 52,
                version: 0,
            }),
        };

        let mut out = Vec::new();
        type_info.put(&mut out).unwrap();
        let mut input = out.as_slice();

        assert_eq!(type_info, TypeInfo::get(&mut input).unwrap());
        assert!(input.is_empty());
    }

    #[test]
    fn put_value_uses_old_byte_length_null_sentinel() {
        let type_info = TypeInfo::new(DataType::IntN, 4);
        let mut out = Vec::new();

        type_info.put_value(&mut out, Option::<i32>::None).unwrap();

        assert_eq!([0xff], out.as_slice());
    }

    #[test]
    fn put_value_and_get_value_round_trip_max_nvarchar_blob() {
        let type_info = TypeInfo::new(DataType::NVarChar, 0xffff);
        let mut out = Vec::new();

        type_info.put_value(&mut out, "hi").unwrap();

        assert_eq!(&[4, 0, 0, 0, 0, 0, 0, 0], &out[..8]);
        assert_eq!(&[4, 0, 0, 0], &out[8..12]);
        assert_eq!(&[0, 0, 0, 0], &out[out.len() - 4..]);

        let mut input = out.as_slice();
        assert_eq!(
            Some(vec![b'h', 0, b'i', 0]),
            type_info.get_value(&mut input).unwrap()
        );
        assert!(input.is_empty());
    }

    #[test]
    fn formats_tds_type_declarations_like_old_protocol_module() {
        let mut out = String::new();
        TypeInfo::new(DataType::NVarChar, 12).fmt(&mut out);

        assert_eq!("nvarchar(6)", out);
        assert_eq!("NVARCHAR", TypeInfo::new(DataType::NVarChar, 12).name());
    }
}
