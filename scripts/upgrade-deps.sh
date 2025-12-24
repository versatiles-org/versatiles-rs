#!/usr/bin/env bash
cd "$(dirname "$0")/.."
set -e

echo "Update rust toolchain"
rustup update

#echo "check features"
#unused-features analyze

rm Cargo.lock

echo "Upgrade Rust Dependencies"

# to use "cargo upgrade": cargo install cargo-edit
cargo upgrade --incompatible

cargo check --workspace

echo "Upgrade NPM dependencies"
cd versatiles_node
npm install
npm run upgrade
