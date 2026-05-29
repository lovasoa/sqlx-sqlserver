#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

: "${MSSQL_DATABASE_URL:?set MSSQL_DATABASE_URL to run SQL Server integration tests}"

cargo test --locked --features integration-tests --test mssql_smoke --no-run

deadline=$((SECONDS + 120))
while true
do
    output="$(cargo test --locked --features integration-tests --test mssql_smoke \
        connects_and_pings_when_configured -- --exact 2>&1)" && {
        printf '%s\n' "$output"
        break
    }

    printf '%s\n' "$output"

    if ! grep -Eiq 'connection refused|connection reset|timed out|early eof|unexpected eof' <<<"$output"; then
        echo "SQL Server readiness check failed with a non-transient error" >&2
        exit 1
    fi

    if ((SECONDS >= deadline)); then
        echo "SQL Server did not become ready before the timeout" >&2
        exit 1
    fi

    echo "waiting for SQL Server to accept connections..."
    sleep 5
done

cargo test --locked --features integration-tests --test mssql_smoke
