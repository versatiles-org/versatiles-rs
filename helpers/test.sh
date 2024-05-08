#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

cargo fmt --check

cargo clippy -p versatiles-lib
cargo clippy -p versatiles-lib -F http
cargo clippy -p versatiles-lib -F full
cargo clippy -p versatiles

cargo test -p versatiles-lib
cargo test -p versatiles-lib -F http
cargo test -p versatiles-lib -F full
cargo test -p versatiles
