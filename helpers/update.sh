#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

echo "Update rust"
rustup update

echo "Find unused dependencies"
cargo +nightly udeps --all-targets --no-default-features
cargo +nightly udeps --all-targets --no-default-features --features mbtiles

#echo "check features"
#unused-features analyze

rm Cargo.lock

echo "upgrade dependencies"
# cargo install cargo-edit
cargo upgrade
