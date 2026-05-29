use sqlx_sqlserver::MssqlConnectOptions;
#[cfg(feature = "integration-tests")]
use std::sync::Once;

#[cfg(feature = "integration-tests")]
use sqlx_core::column::Column;
#[cfg(feature = "integration-tests")]
use sqlx_core::connection::{ConnectOptions, Connection};
#[cfg(feature = "integration-tests")]
use sqlx_core::executor::Executor;
#[cfg(feature = "integration-tests")]
use sqlx_core::row::Row;
#[cfg(all(feature = "integration-tests", feature = "migrate"))]
use sqlx_core::sql_str::SqlSafeStr;
#[cfg(feature = "integration-tests")]
use sqlx_core::statement::Statement;
#[cfg(feature = "integration-tests")]
use sqlx_core::value::ValueRef;
#[cfg(all(feature = "integration-tests", feature = "migrate"))]
use std::borrow::Cow;

fn database_url() -> Option<String> {
    std::env::var("MSSQL_DATABASE_URL")
        .ok()
        .and_then(|url| match url.trim() {
            "" => None,
            url => Some(url.to_owned()),
        })
}

#[cfg(feature = "integration-tests")]
async fn any_test_conn(
    test_name: &str,
) -> Result<Option<sqlx_core::any::AnyConnection>, Box<dyn std::error::Error>> {
    static INSTALL: Once = Once::new();

    let Some(url) = database_url() else {
        eprintln!("skipping {test_name}: MSSQL_DATABASE_URL is not set");
        return Ok(None);
    };

    INSTALL.call_once(|| {
        sqlx_core::any::driver::install_drivers(&[sqlx_sqlserver::any::DRIVER])
            .expect("SQL Server Any driver should install once");
    });

    Ok(Some(sqlx_core::any::AnyConnection::connect(&url).await?))
}

#[cfg(all(feature = "integration-tests", feature = "migrate"))]
async fn any_migrate_test_conn(
    test_name: &str,
) -> Result<Option<sqlx_core::any::AnyConnection>, Box<dyn std::error::Error>> {
    any_test_conn(test_name).await
}

#[cfg(feature = "integration-tests")]
async fn native_test_conn(
    test_name: &str,
) -> Result<Option<sqlx_sqlserver::MssqlConnection>, Box<dyn std::error::Error>> {
    let Some(url) = database_url() else {
        eprintln!("skipping {test_name}: MSSQL_DATABASE_URL is not set");
        return Ok(None);
    };

    let options = url.parse::<MssqlConnectOptions>()?;
    Ok(Some(options.connect().await?))
}

#[test]
fn mssql_database_url_is_parseable_when_set() {
    let Some(url) = database_url() else {
        eprintln!("skipping SQL Server smoke test: MSSQL_DATABASE_URL is not set");
        return;
    };

    url.parse::<MssqlConnectOptions>()
        .expect("MSSQL_DATABASE_URL should parse as SQL Server options");
}

#[cfg(feature = "migrate")]
#[test]
fn any_driver_exposes_migrate_database_when_feature_enabled() {
    sqlx_sqlserver::any::DRIVER
        .get_migrate_database()
        .expect("SQL Server Any driver should expose migration database hooks");
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn connects_and_pings_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server integration test").await? else {
        return Ok(());
    };

    conn.ping().await?;
    conn.close().await?;

    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_select_one_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server query test").await? else {
        return Ok(());
    };

    let row = sqlx_core::query::query("SELECT 1")
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(1_i32, row.try_get::<i32, _>(0)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runs_independent_connections_in_parallel_when_configured(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = database_url() else {
        eprintln!("skipping SQL Server async parallelism test: MSSQL_DATABASE_URL is not set");
        return Ok(());
    };

    let mut tasks = Vec::new();

    for expected in 0_i32..8 {
        let url = url.clone();
        tasks.push(tokio::spawn(async move {
            let options = url.parse::<MssqlConnectOptions>().map_err(|error| {
                format!("failed to parse MSSQL_DATABASE_URL for parallel task {expected}: {error}")
            })?;
            let mut conn = options.connect().await.map_err(|error| {
                format!("failed to connect SQL Server parallel task {expected}: {error}")
            })?;
            let row = sqlx_core::query::query("SELECT @p1")
                .bind(expected)
                .fetch_one(&mut conn)
                .await
                .map_err(|error| format!("parallel SQL Server query {expected} failed: {error}"))?;
            let actual = row.try_get::<i32, _>(0).map_err(|error| {
                format!("parallel SQL Server decode {expected} failed: {error}")
            })?;
            conn.close().await.map_err(|error| {
                format!("failed to close SQL Server parallel task {expected}: {error}")
            })?;

            if actual != expected {
                return Err(format!(
                    "parallel SQL Server task returned {actual}, expected {expected}"
                ));
            }

            Ok::<(), String>(())
        }));
    }

    for task in tasks {
        if let Err(message) = task.await? {
            return Err(std::io::Error::other(message).into());
        }
    }

    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_bound_scalars_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server bound scalar test").await? else {
        return Ok(());
    };

    let row = sqlx_core::query::query("SELECT @p1, @p2")
        .bind(7_i32)
        .bind("hello")
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(7_i32, row.try_get::<i32, _>(0)?);
    assert_eq!("hello", row.try_get::<String, _>(1)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_bound_large_text_and_binary_when_configured(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server large parameter test").await? else {
        return Ok(());
    };

    let text = "x".repeat(4001);
    let row = sqlx_core::query::query("SELECT LEN(@p1), DATALENGTH(@p1)")
        .bind(text.as_str())
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(4001_i64, row.try_get::<i64, _>(0)?);
    assert_eq!(8002_i64, row.try_get::<i64, _>(1)?);

    let bytes = vec![0x5a; 8001];
    let row = sqlx_core::query::query("SELECT DATALENGTH(@p1)")
        .bind(bytes.as_slice())
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(8001_i64, row.try_get::<i64, _>(0)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_native_scalar_arguments_when_configured() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(mut conn) = native_test_conn("SQL Server native scalar argument test").await? else {
        return Ok(());
    };

    let bytes = vec![0x01, 0x23, 0x45, 0x67];
    let row = sqlx_core::query::query(
        "SELECT @p1, @p2, @p3, @p4, @p5, @p6, CAST(@p7 AS VARCHAR(12)), CAST(@p8 AS NVARCHAR(12))",
    )
    .bind(true)
    .bind(-1234_i16)
    .bind(9_876_543_210_i64)
    .bind(3.25_f32)
    .bind(6.5_f64)
    .bind(bytes.as_slice())
    .bind("varchar")
    .bind("nvarchar")
    .fetch_one(&mut conn)
    .await?;

    assert!(row.try_get::<bool, _>(0)?);
    assert_eq!(-1234_i16, row.try_get::<i16, _>(1)?);
    assert_eq!(9_876_543_210_i64, row.try_get::<i64, _>(2)?);
    assert_eq!(3.25_f32, row.try_get::<f32, _>(3)?);
    assert_eq!(6.5_f64, row.try_get::<f64, _>(4)?);
    assert_eq!(bytes, row.try_get::<Vec<u8>, _>(5)?);
    assert_eq!("varchar", row.try_get::<String, _>(6)?);
    assert_eq!("nvarchar", row.try_get::<String, _>(7)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_varchar_text_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server varchar decode test").await? else {
        return Ok(());
    };

    let row = sqlx_core::query::query("SELECT CAST('hello' AS VARCHAR(5))")
        .fetch_one(&mut conn)
        .await?;

    assert_eq!("hello", row.try_get::<String, _>(0)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_bound_null_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server bound null test").await? else {
        return Ok(());
    };

    let row = sqlx_core::query::query("SELECT @p1")
        .bind(Option::<i32>::None)
        .fetch_one(&mut conn)
        .await?;

    assert!(row.try_get_raw(0)?.is_null());

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn prepares_statement_metadata_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server prepare test").await? else {
        return Ok(());
    };

    let statement = conn
        .prepare(sqlx_core::sql_str::SqlStr::from_static("SELECT 1 AS value"))
        .await?;

    assert_eq!(1, statement.columns().len());
    assert_eq!("value", statement.columns()[0].name());

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn rolls_back_transaction_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server transaction test").await? else {
        return Ok(());
    };

    let tx = conn.begin().await?;
    tx.rollback().await?;

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn dropped_transaction_rolls_back_on_next_use() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server dropped transaction rollback test").await?
    else {
        return Ok(());
    };

    let table_name = "_sqlx_drop_rollback_smoke";
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}"
        )))
        .await?;
    conn.execute(sqlx_core::sql_str::AssertSqlSafe(format!(
        "CREATE TABLE {table_name} (id INT NOT NULL)"
    )))
    .await?;

    {
        let mut tx = conn.begin().await?;
        sqlx_core::query::query(sqlx_core::sql_str::AssertSqlSafe(format!(
            "INSERT INTO {table_name} (id) VALUES (1)"
        )))
        .execute(&mut *tx)
        .await?;
    }

    let count: i32 = sqlx_core::query_scalar::query_scalar(sqlx_core::sql_str::AssertSqlSafe(
        format!("SELECT COUNT(*) FROM {table_name}"),
    ))
    .fetch_one(&mut conn)
    .await?;

    assert_eq!(0, count);

    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}"
        )))
        .await?;
    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn any_fetches_bound_integer_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = any_test_conn("SQL Server Any bound argument test").await? else {
        return Ok(());
    };

    let row = sqlx_core::query::query("SELECT @p1")
        .bind(7_i32)
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(7_i32, row.try_get::<i32, _>(0)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn any_fetches_text_blob_bool_and_typed_null_arguments_when_configured(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = any_test_conn("SQL Server Any text/blob/bool/null argument test").await?
    else {
        return Ok(());
    };

    let bytes = vec![0xde, 0xad, 0xbe, 0xef];
    let row = sqlx_core::query::query("SELECT @p1, @p2, @p3, @p4")
        .bind("hello any")
        .bind(bytes.as_slice())
        .bind(true)
        .bind(Option::<String>::None)
        .fetch_one(&mut conn)
        .await?;

    assert_eq!("hello any", row.try_get::<String, _>(0)?);
    assert_eq!(bytes, row.try_get::<Vec<u8>, _>(1)?);
    assert!(row.try_get::<bool, _>(2)?);
    assert!(row.try_get_raw(3)?.is_null());

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn any_prepares_statement_metadata_when_configured() -> Result<(), Box<dyn std::error::Error>>
{
    let Some(mut conn) = any_test_conn("SQL Server Any prepare test").await? else {
        return Ok(());
    };

    let statement = conn
        .prepare(sqlx_core::sql_str::SqlStr::from_static("SELECT 1 AS value"))
        .await?;

    assert_eq!(1, statement.columns().len());
    assert_eq!("value", statement.columns()[0].name());

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn any_fetches_select_one_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = any_test_conn("SQL Server Any query test").await? else {
        return Ok(());
    };

    let row = sqlx_core::query::query("SELECT 1")
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(1_i32, row.try_get::<i32, _>(0)?);

    conn.close().await?;
    Ok(())
}

#[cfg(all(feature = "integration-tests", feature = "migrate"))]
#[tokio::test]
async fn runs_migration_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = native_test_conn("SQL Server migration test").await? else {
        return Ok(());
    };

    let table_name = "_sqlx_migrations_smoke";
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}_table"
        )))
        .await?;
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}"
        )))
        .await?;

    let migration = sqlx_core::migrate::Migration::new(
        1,
        Cow::Borrowed("create smoke table"),
        sqlx_core::migrate::MigrationType::Simple,
        sqlx_core::sql_str::AssertSqlSafe(format!(
            "CREATE TABLE {table_name}_table (id INT NOT NULL)"
        ))
        .into_sql_str(),
        false,
    );
    let mut migrator = sqlx_core::migrate::Migrator::with_migrations(vec![migration]);
    migrator.dangerous_set_table_name(table_name.to_owned());

    migrator.run_direct(None, &mut conn, false).await?;

    let exists: bool = sqlx_core::query_scalar::query_scalar(
        "SELECT CONVERT(bit, CASE WHEN OBJECT_ID(N'_sqlx_migrations_smoke_table', N'U') IS NULL THEN 0 ELSE 1 END)",
    )
    .fetch_one(&mut conn)
    .await?;

    assert!(exists);

    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}_table"
        )))
        .await?;
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}"
        )))
        .await?;
    conn.close().await?;

    Ok(())
}

#[cfg(all(feature = "integration-tests", feature = "migrate"))]
#[tokio::test]
async fn any_runs_migration_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(mut conn) = any_migrate_test_conn("SQL Server Any migration test").await? else {
        return Ok(());
    };

    let table_name = "_sqlx_any_migrations_smoke";
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}_table"
        )))
        .await?;
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}"
        )))
        .await?;

    let migration = sqlx_core::migrate::Migration::new(
        1,
        Cow::Borrowed("create any smoke table"),
        sqlx_core::migrate::MigrationType::Simple,
        sqlx_core::sql_str::AssertSqlSafe(format!(
            "CREATE TABLE {table_name}_table (id INT NOT NULL)"
        ))
        .into_sql_str(),
        false,
    );
    let mut migrator = sqlx_core::migrate::Migrator::with_migrations(vec![migration]);
    migrator.dangerous_set_table_name(table_name.to_owned());

    migrator.run_direct(None, &mut conn, false).await?;

    let exists: bool = sqlx_core::query_scalar::query_scalar(
        "SELECT CONVERT(bit, CASE WHEN OBJECT_ID(N'_sqlx_any_migrations_smoke_table', N'U') IS NULL THEN 0 ELSE 1 END)",
    )
    .fetch_one(&mut conn)
    .await?;

    assert!(exists);

    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}_table"
        )))
        .await?;
    let _ = conn
        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
            "DROP TABLE IF EXISTS {table_name}"
        )))
        .await?;
    conn.close().await?;

    Ok(())
}
