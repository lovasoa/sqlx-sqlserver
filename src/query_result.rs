/// Summary of a SQL Server query execution.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MssqlQueryResult {
    rows_affected: u64,
}

impl MssqlQueryResult {
    /// Creates a query result with a row count.
    pub const fn new(rows_affected: u64) -> Self {
        Self { rows_affected }
    }

    /// Returns the number of rows affected.
    pub const fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

impl Extend<Self> for MssqlQueryResult {
    fn extend<T: IntoIterator<Item = Self>>(&mut self, iter: T) {
        self.rows_affected += iter
            .into_iter()
            .map(|result| result.rows_affected)
            .sum::<u64>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_results_sum_rows_affected() {
        let mut result = MssqlQueryResult::new(2);
        result.extend([MssqlQueryResult::new(3), MssqlQueryResult::new(5)]);

        assert_eq!(10, result.rows_affected());
    }
}
