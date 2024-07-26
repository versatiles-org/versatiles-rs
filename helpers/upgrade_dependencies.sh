#!/usr/bin/env bash
cd "$(dirname "$0")/.."
set -e

echo "Update Rust"
rustup update

#echo "check features"
#unused-features analyze

rm Cargo.lock

echo "Upgrade Dependencies"

# to use "cargo upgrade": cargo install cargo-edit
cargo upgrade --incompatible

cargo check --workspace
