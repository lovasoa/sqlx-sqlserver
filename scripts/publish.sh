#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

: "${CARGO_REGISTRY_TOKEN:?set CARGO_REGISTRY_TOKEN to publish to crates.io}"

cargo publish --locked
