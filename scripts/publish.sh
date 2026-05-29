#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

dry_run=false
for arg in "$@"; do
    if [[ "$arg" == "--dry-run" || "$arg" == "-n" ]]; then
        dry_run=true
    fi
done

if [[ "$dry_run" == false ]]; then
    : "${CARGO_REGISTRY_TOKEN:?set CARGO_REGISTRY_TOKEN to publish to crates.io}"
fi

cargo publish --locked "$@"
