use futures_core::future::BoxFuture;
use sqlx_core::connection::{ConnectOptions, Connection};
use sqlx_core::error::Error;
use sqlx_core::executor::Executor;
use sqlx_core::migrate::{AppliedMigration, Migrate, MigrateDatabase, MigrateError, Migration};
use sqlx_core::query::query;
use sqlx_core::query_as::query_as;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::sql_str::AssertSqlSafe;
use std::str::FromStr;
use std::time::{Duration, Instant};

use crate::{Mssql, MssqlConnectOptions, MssqlConnection};

fn parse_for_maintenance(url: &str) -> Result<(MssqlConnectOptions, String), Error> {
    let mut options = MssqlConnectOptions::from_str(url)?;
    let database = options.database().to_owned();

    options.set_database_for_maintenance();

    Ok((options, database))
}

impl MigrateDatabase for Mssql {
    async fn create_database(url: &str) -> Result<(), Error> {
        let (options, database) = parse_for_maintenance(url)?;
        let mut conn = options.connect().await?;

        conn.execute(AssertSqlSafe(format!(
            "CREATE DATABASE {}",
            quote_ident(&database)
        )))
        .await?;

        conn.close().await
    }

    async fn database_exists(url: &str) -> Result<bool, Error> {
        let (options, database) = parse_for_maintenance(url)?;
        let mut conn = options.connect().await?;

        let exists: bool =
            query_scalar("SELECT CONVERT(bit, CASE WHEN DB_ID(@p1) IS NULL THEN 0 ELSE 1 END)")
                .bind(database)
                .fetch_one(&mut conn)
                .await?;

        conn.close().await?;
        Ok(exists)
    }

    async fn drop_database(url: &str) -> Result<(), Error> {
        let (options, database) = parse_for_maintenance(url)?;
        let mut conn = options.connect().await?;

        conn.execute(AssertSqlSafe(format!(
            "IF DB_ID(N'{}') IS NOT NULL DROP DATABASE {}",
            quote_string(&database),
            quote_ident(&database)
        )))
        .await?;

        conn.close().await
    }
}

impl Migrate for MssqlConnection {
    fn create_schema_if_not_exists<'e>(
        &'e mut self,
        schema_name: &'e str,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        Box::pin(async move {
            self.execute(AssertSqlSafe(format!(
                "IF SCHEMA_ID(N'{}') IS NULL EXEC(N'CREATE SCHEMA {}')",
                quote_string(schema_name),
                quote_string(&quote_ident(schema_name))
            )))
            .await?;

            Ok(())
        })
    }

    fn ensure_migrations_table<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        Box::pin(async move {
            self.execute(AssertSqlSafe(format!(
                r#"
IF OBJECT_ID(N'{object_name}', N'U') IS NULL
BEGIN
    CREATE TABLE {table_name} (
        version BIGINT PRIMARY KEY,
        description NVARCHAR(MAX) NOT NULL,
        installed_on DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
        success BIT NOT NULL,
        checksum VARBINARY(MAX) NOT NULL,
        execution_time BIGINT NOT NULL
    );
END
                "#,
                object_name = quote_string(table_name),
                table_name = table_name
            )))
            .await?;

            Ok(())
        })
    }

    fn dirty_version<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<Option<i64>, MigrateError>> {
        Box::pin(async move {
            let row: Option<(i64,)> = query_as(AssertSqlSafe(format!(
                "SELECT TOP (1) version FROM {table_name} WHERE success = 0 ORDER BY version"
            )))
            .fetch_optional(self)
            .await?;

            Ok(row.map(|r| r.0))
        })
    }

    fn list_applied_migrations<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<Vec<AppliedMigration>, MigrateError>> {
        Box::pin(async move {
            let rows: Vec<(i64, Vec<u8>)> = query_as(AssertSqlSafe(format!(
                "SELECT version, checksum FROM {table_name} ORDER BY version"
            )))
            .fetch_all(self)
            .await?;

            Ok(rows
                .into_iter()
                .map(|(version, checksum)| AppliedMigration {
                    version,
                    checksum: checksum.into(),
                })
                .collect())
        })
    }

    fn lock(&mut self) -> BoxFuture<'_, Result<(), MigrateError>> {
        Box::pin(async move {
            let _: Option<(i32,)> = query_as(
                r#"
DECLARE @result int;
EXEC @result = sp_getapplock
    @Resource = N'sqlx-migrate',
    @LockMode = N'Exclusive',
    @LockOwner = N'Session',
    @LockTimeout = -1;
SELECT @result;
                "#,
            )
            .fetch_optional(self)
            .await?;

            Ok(())
        })
    }

    fn unlock(&mut self) -> BoxFuture<'_, Result<(), MigrateError>> {
        Box::pin(async move {
            query(
                r#"
EXEC sp_releaseapplock
    @Resource = N'sqlx-migrate',
    @LockOwner = N'Session';
                "#,
            )
            .execute(self)
            .await?;

            Ok(())
        })
    }

    fn apply<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<Duration, MigrateError>> {
        Box::pin(async move {
            let start = Instant::now();

            if migration.no_tx {
                execute_migration(self, table_name, migration).await?;
            } else {
                let mut tx = self.begin().await?;
                execute_migration(&mut tx, table_name, migration).await?;
                tx.commit().await?;
            }

            let elapsed = start.elapsed();

            #[allow(clippy::cast_possible_truncation)]
            query(AssertSqlSafe(format!(
                r#"
UPDATE {table_name}
SET execution_time = @p1
WHERE version = @p2
                "#
            )))
            .bind(elapsed.as_nanos() as i64)
            .bind(migration.version)
            .execute(self)
            .await?;

            Ok(elapsed)
        })
    }

    fn revert<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<Duration, MigrateError>> {
        Box::pin(async move {
            let start = Instant::now();

            if migration.no_tx {
                revert_migration(self, table_name, migration).await?;
            } else {
                let mut tx = self.begin().await?;
                revert_migration(&mut tx, table_name, migration).await?;
                tx.commit().await?;
            }

            Ok(start.elapsed())
        })
    }

    fn skip<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        Box::pin(async move {
            query(AssertSqlSafe(format!(
                r#"
INSERT INTO {table_name} ( version, description, success, checksum, execution_time )
VALUES ( @p1, @p2, 1, @p3, -1 )
                "#
            )))
            .bind(migration.version)
            .bind(&*migration.description)
            .bind(&*migration.checksum)
            .execute(self)
            .await?;

            Ok(())
        })
    }
}

async fn execute_migration(
    conn: &mut MssqlConnection,
    table_name: &str,
    migration: &Migration,
) -> Result<(), MigrateError> {
    conn.execute(migration.sql.clone())
        .await
        .map_err(|error| MigrateError::ExecuteMigration(error, migration.version))?;

    query(AssertSqlSafe(format!(
        r#"
INSERT INTO {table_name} ( version, description, success, checksum, execution_time )
VALUES ( @p1, @p2, 1, @p3, -1 )
        "#
    )))
    .bind(migration.version)
    .bind(&*migration.description)
    .bind(&*migration.checksum)
    .execute(conn)
    .await?;

    Ok(())
}

async fn revert_migration(
    conn: &mut MssqlConnection,
    table_name: &str,
    migration: &Migration,
) -> Result<(), MigrateError> {
    conn.execute(migration.sql.clone())
        .await
        .map_err(|error| MigrateError::ExecuteMigration(error, migration.version))?;

    query(AssertSqlSafe(format!(
        "DELETE FROM {table_name} WHERE version = @p1"
    )))
    .bind(migration.version)
    .execute(conn)
    .await?;

    Ok(())
}

fn quote_ident(identifier: &str) -> String {
    format!("[{}]", identifier.replace(']', "]]"))
}

fn quote_string(value: &str) -> String {
    value.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_sql_server_identifiers() {
        assert_eq!("[db]]name]", quote_ident("db]name"));
    }

    #[test]
    fn quotes_t_sql_strings() {
        assert_eq!("can''t", quote_string("can't"));
    }
}
