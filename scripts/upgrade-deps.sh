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

# fuzz/ is a workspace of its own, untouched by the commands above. No check:
# its targets only compile under `cargo fuzz`, which sets --cfg fuzzing.
echo "Upgrade Fuzz Dependencies"
cargo upgrade --incompatible --manifest-path fuzz/Cargo.toml
cargo update --manifest-path fuzz/Cargo.toml

echo "Upgrade NPM dependencies"
cd versatiles_node
npm install
npm run upgrade
