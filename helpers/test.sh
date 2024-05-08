#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

cargo fmt --check

cargo clippy --bins $1
cargo clippy --lib --no-default-features $1
cargo clippy --lib --no-default-features -F http $1
cargo clippy --lib --no-default-features -F full $1

cargo test --bins $1
cargo test --lib --no-default-features $1
cargo test --lib --no-default-features -F http $1
cargo test --lib --no-default-features -F full $1
