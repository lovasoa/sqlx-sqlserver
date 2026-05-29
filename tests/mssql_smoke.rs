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
