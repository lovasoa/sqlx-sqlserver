use sqlx_core::Error;

use super::read::{read_u16_le, read_u64_le};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Done {
    pub(crate) status: Status,
    #[allow(dead_code)]
    cursor_command: u16,
    pub(crate) affected_rows: u64,
}

impl Done {
    pub(crate) fn get(input: &mut &[u8]) -> Result<Self, Error> {
        Ok(Self {
            status: Status::from_bits_truncate(read_u16_le(input)?),
            cursor_command: read_u16_le(input)?,
            affected_rows: read_u64_le(input)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Status(u16);

impl Status {
    pub(crate) const DONE_COUNT: Self = Self(0x0010);

    pub(crate) const fn from_bits_truncate(bits: u16) -> Self {
        Self(bits & 0x0117)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_done_with_affected_rows() {
        let mut input = &[0x10, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0][..];
        let done = Done::get(&mut input).unwrap();

        assert!(done.status.contains(Status::DONE_COUNT));
        assert_eq!(done.affected_rows, 7);
    }
}
