use sqlx_sqlserver::MssqlConnectOptions;
#[cfg(feature = "integration-tests")]
use std::sync::Once;

#[cfg(feature = "integration-tests")]
use sqlx_core::connection::{ConnectOptions, Connection};
#[cfg(feature = "integration-tests")]
use sqlx_core::row::Row;
#[cfg(feature = "integration-tests")]
use sqlx_core::value::ValueRef;

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
