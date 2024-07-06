#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo -e "\033[1;33munused dependencies for binary\033[0m"
cargo +nightly udeps -q --bins

echo -e "\033[1;33munused dependencies for library (minimal)\033[0m"
cargo +nightly udeps -q --lib --workspace --no-default-features

echo -e "\033[1;33munused dependencies for versatiles_container (cli)\033[0m"
cargo +nightly udeps -q -p versatiles_container --lib --no-default-features --features cli

echo -e "\033[1;33munused dependencies for versatiles_container (test)\033[0m"
cargo +nightly udeps -q -p versatiles_container --lib --no-default-features --features test

echo -e "\033[1;33munused dependencies for versatiles_core (cli)\033[0m"
cargo +nightly udeps -q -p versatiles_core --lib --no-default-features --features cli

echo -e "\033[1;33munused dependencies for versatiles_core (test)\033[0m"
cargo +nightly udeps -q -p versatiles_core --lib --no-default-features --features test
