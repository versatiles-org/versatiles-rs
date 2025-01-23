#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo -e "\033[1;33munused dependencies for binary\033[0m"
cargo +nightly udeps -q --bins

echo -e "\033[1;33munused dependencies for library (minimal)\033[0m"
cargo +nightly udeps -q --lib --workspace --no-default-features

echo -e "\033[1;33munused dependencies for library (cli)\033[0m"
cargo +nightly udeps -q --lib --workspace --no-default-features --features cli --exclude versatiles

echo -e "\033[1;33munused dependencies for library (test)\033[0m"
cargo +nightly udeps -q --lib --workspace --no-default-features --features test

echo -e "\033[1;33munused dependencies for library (all-features)\033[0m"
cargo +nightly udeps -q --lib --workspace --all-features --exclude versatiles --exclude versatiles_core
