# sqlx-sqlserver

Independent Microsoft SQL Server driver crate for SQLx.

This crate is a standalone top-level crate. It depends only on crates published
to crates.io and is not wired into the `sqlx` facade crate, so examples use
`sqlx-core` directly.

## Minimal Query

```toml
[dependencies]
sqlx-sqlserver = "0.0.1-alpha"
sqlx-core = "=0.9.0"
tokio = { version = "1", features = ["macros", "rt"] }
```

```rust
use sqlx_core::connection::{ConnectOptions, Connection};
use sqlx_core::row::Row;
use sqlx_sqlserver::MssqlConnectOptions;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = "mssql://sa:Password123!@localhost:1433/master?encrypt=mandatory&trust_server_certificate=true"
        .parse::<MssqlConnectOptions>()?
        .connect()
        .await?;

    let row = sqlx_core::query::query("SELECT 1")
        .fetch_one(&mut conn)
        .await?;

    let value: i32 = row.try_get(0)?;
    println!("{value}");

    conn.close().await?;
    Ok(())
}
```

For local development, run fast tests with `./scripts/ci.sh`. Run e2e tests
with `MSSQL_DATABASE_URL=... ./scripts/test-mssql.sh`.
