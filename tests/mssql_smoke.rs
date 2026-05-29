use sqlx_sqlserver::MssqlConnectOptions;
#[cfg(feature = "integration-tests")]
use std::sync::Once;

#[cfg(feature = "integration-tests")]
use sqlx_core::connection::{ConnectOptions, Connection};
#[cfg(feature = "integration-tests")]
use sqlx_core::row::Row;

fn database_url() -> Option<String> {
    let _ = dotenvy::dotenv();
    std::env::var("MSSQL_DATABASE_URL").ok()
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

#[test]
fn mssql_database_url_is_parseable_when_set() {
    let Some(url) = database_url() else {
        eprintln!("skipping SQL Server smoke test: MSSQL_DATABASE_URL is not set");
        return;
    };

    url.parse::<MssqlConnectOptions>()
        .expect("MSSQL_DATABASE_URL should parse as SQL Server options");
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn connects_and_pings_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = database_url() else {
        eprintln!("skipping SQL Server integration test: MSSQL_DATABASE_URL is not set");
        return Ok(());
    };

    let options = url.parse::<MssqlConnectOptions>()?;
    let mut conn = options.connect().await?;
    conn.ping().await?;
    conn.close().await?;

    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn fetches_select_one_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = database_url() else {
        eprintln!("skipping SQL Server query test: MSSQL_DATABASE_URL is not set");
        return Ok(());
    };

    let options = url.parse::<MssqlConnectOptions>()?;
    let mut conn = options.connect().await?;
    let row = sqlx_core::query::query("SELECT 1")
        .fetch_one(&mut conn)
        .await?;

    assert_eq!(1_i32, row.try_get::<i32, _>(0)?);

    conn.close().await?;
    Ok(())
}

#[cfg(feature = "integration-tests")]
#[tokio::test]
async fn rolls_back_transaction_when_configured() -> Result<(), Box<dyn std::error::Error>> {
    let Some(url) = database_url() else {
        eprintln!("skipping SQL Server transaction test: MSSQL_DATABASE_URL is not set");
        return Ok(());
    };

    let options = url.parse::<MssqlConnectOptions>()?;
    let mut conn = options.connect().await?;
    let tx = conn.begin().await?;
    tx.rollback().await?;

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
