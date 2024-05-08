#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

cargo fmt --check

cargo clippy --bins
cargo clippy --lib --no-default-features
cargo clippy --lib --no-default-features -F http
cargo clippy --lib --no-default-features -F full

cargo test --bins
cargo test --lib --no-default-features
cargo test --lib --no-default-features -F http
cargo test --lib --no-default-features -F full
