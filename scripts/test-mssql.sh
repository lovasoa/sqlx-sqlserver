#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

: "${MSSQL_DATABASE_URL:?set MSSQL_DATABASE_URL to run SQL Server integration tests}"

cargo test --locked --features integration-tests --test mssql_smoke --no-run

deadline=$((SECONDS + 120))
until cargo test --locked --features integration-tests --test mssql_smoke \
    connects_and_pings_when_configured -- --exact
do
    if ((SECONDS >= deadline)); then
        echo "SQL Server did not become ready before the timeout" >&2
        exit 1
    fi

    echo "waiting for SQL Server to accept connections..."
    sleep 5
done

cargo test --locked --features integration-tests --test mssql_smoke
