#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

cargo fmt

cargo clippy --quiet --bins $1
cargo clippy --quiet --lib --no-default-features $1
cargo clippy --quiet --lib --no-default-features -F http $1
cargo clippy --quiet --lib --no-default-features -F full $1

cargo test --quiet --bins $1
cargo test --quiet --lib --no-default-features $1
cargo test --quiet --lib --no-default-features -F http $1
cargo test --quiet --lib --no-default-features -F full $1
