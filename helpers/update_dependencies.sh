#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo "Update rust"
rustup update

#echo "check features"
#unused-features analyze

rm Cargo.lock

echo "upgrade dependencies"
# cargo install cargo-edit
cargo upgrade

cargo check --bin versatiles --all-features
