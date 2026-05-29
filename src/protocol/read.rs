use sqlx_core::Error;

pub(crate) fn read_u8(input: &mut &[u8]) -> Result<u8, Error> {
    Ok(take(input, 1)?[0])
}

pub(crate) fn read_u16_le(input: &mut &[u8]) -> Result<u16, Error> {
    let bytes = take(input, 2)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

pub(crate) fn read_u32_le(input: &mut &[u8]) -> Result<u32, Error> {
    let bytes = take(input, 4)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

pub(crate) fn read_u64_le(input: &mut &[u8]) -> Result<u64, Error> {
    let bytes = take(input, 8)?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

pub(crate) fn read_len_prefixed<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], Error> {
    let len = usize::from(read_u16_le(input)?);
    take(input, len)
}

pub(crate) fn read_b_varchar(input: &mut &[u8]) -> Result<String, Error> {
    let len_chars = usize::from(read_u8(input)?);
    read_utf16(input, len_chars)
}

pub(crate) fn read_utf16(input: &mut &[u8], len_chars: usize) -> Result<String, Error> {
    let len_bytes = len_chars
        .checked_mul(2)
        .ok_or_else(|| Error::Protocol("SQL Server string length overflow".to_owned()))?;
    let bytes = take(input, len_bytes)?;
    let units = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();

    String::from_utf16(&units)
        .map_err(|_| Error::Protocol("SQL Server string contained invalid UTF-16".to_owned()))
}

pub(crate) fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], Error> {
    let bytes = input.get(..len).ok_or_else(|| {
        Error::Protocol("SQL Server query token ended before expected length".to_owned())
    })?;
    *input = &input[len..];
    Ok(bytes)
}
