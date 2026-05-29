# sqlx-sqlserver tests

This crate is independent of the local SQLx workspace. Do not add workspace
membership or `path =` dependencies.

## Fast tests

Run the tests that do not require SQL Server:

```sh
cargo test
```

While the port is incomplete, add coverage here first for URL parsing, SSRP/TDS
packet helpers, metadata/type helpers, and argument encoding.

## Integration tests

Integration tests require `MSSQL_DATABASE_URL`. When the variable is absent, the
tests print a skip message and pass.

```sh
MSSQL_DATABASE_URL='mssql://sa:Password123!@localhost:1433/master?encrypt=not_supported' \
cargo test --features integration-tests --test mssql_smoke
```

The current wire slice supports unencrypted login only. TLS pre-login support is
still pending, so use `encrypt=not_supported` only with development servers that
allow unencrypted login.
