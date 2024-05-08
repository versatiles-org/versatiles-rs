#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

cargo fmt --check

cargo clippy -p versatiles
cargo clippy -p versatiles-lib --no-default-features
cargo clippy -p versatiles-lib --no-default-features -F http
cargo clippy -p versatiles-lib --no-default-features -F full

cargo test -p versatiles
cargo test -p versatiles-lib --no-default-features
cargo test -p versatiles-lib --no-default-features -F http
cargo test -p versatiles-lib --no-default-features -F full
