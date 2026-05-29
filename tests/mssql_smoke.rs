use sqlx_sqlserver::MssqlConnectOptions;

#[cfg(feature = "integration-tests")]
use sqlx_core::connection::{ConnectOptions, Connection};

fn database_url() -> Option<String> {
    let _ = dotenvy::dotenv();
    std::env::var("MSSQL_DATABASE_URL").ok()
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
