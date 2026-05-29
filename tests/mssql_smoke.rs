use sqlx_sqlserver::MssqlConnectOptions;

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
async fn integration_placeholder_skips_until_wire_connection_lands() {
    let Some(_url) = database_url() else {
        eprintln!("skipping SQL Server integration test: MSSQL_DATABASE_URL is not set");
        return;
    };

    eprintln!("skipping SQL Server wire smoke test: connection port is not implemented yet");
}
