# sqlx-sqlserver tests

This crate is independent of the local SQLx workspace. Do not add workspace
membership or `path =` dependencies.

## Fast tests

Run the tests that do not require SQL Server:

```sh
./scripts/ci.sh
```

While the port is incomplete, add coverage here first for URL parsing, SSRP/TDS
packet helpers, metadata/type helpers, and argument encoding.

## Integration tests

Integration tests require `MSSQL_DATABASE_URL`. When the variable is absent, the
tests print a skip message and pass.

```sh
MSSQL_DATABASE_URL='mssql://sa:Password123!@localhost:1433/master?encrypt=mandatory&trust_server_certificate=true' \
./scripts/test-mssql.sh
```

Encrypted connections use the SQL Server PRELOGIN-wrapped TLS handshake. Use
`encrypt=not_supported` only with development servers that explicitly allow
plaintext login.
