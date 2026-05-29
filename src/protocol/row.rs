use sqlx_core::column::Column;
use sqlx_core::Error;
use std::sync::Arc;

use super::read::take;
use crate::{MssqlColumn, MssqlRow, MssqlValue};

#[derive(Debug)]
pub(crate) struct Row;

impl Row {
    pub(crate) fn get(
        input: &mut &[u8],
        nullable: bool,
        columns: Arc<[MssqlColumn]>,
    ) -> Result<MssqlRow, Error> {
        let mut values = Vec::with_capacity(columns.len());
        let nulls = if nullable {
            take(input, columns.len().div_ceil(8))?
        } else {
            &[][..]
        };

        for (index, column) in columns.iter().enumerate() {
            let type_info = column.type_info();

            let value = if nullable && (nulls[index / 8] & (1 << (index % 8))) != 0 {
                MssqlValue::null(type_info.clone())
            } else {
                let protocol_type_info = type_info.protocol_type_info().ok_or_else(|| {
                    Error::Protocol(format!("missing protocol type info for {type_info}"))
                })?;
                let data = protocol_type_info
                    .get_value(input)
                    .map_err(|error| Error::Protocol(error.to_string()))?;
                MssqlValue::new(type_info.clone(), data)
            };

            values.push(value);
        }

        Ok(MssqlRow::new_shared(columns, values))
    }
}
