#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

: "${MSSQL_DATABASE_URL:?set MSSQL_DATABASE_URL to run SQL Server integration tests}"

cargo test --locked --features integration-tests --test mssql_smoke
