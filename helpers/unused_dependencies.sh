#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo -e "\033[1;33munused dependencies for binary\033[0m"
cargo +nightly udeps -q --bins

echo -e "\033[1;33munused dependencies for library (minimal)\033[0m"
cargo +nightly udeps -q --lib --no-default-features

echo -e "\033[1;33munused dependencies for library (http)\033[0m"
cargo +nightly udeps -q --lib --no-default-features --features http

echo -e "\033[1;33munused dependencies for library (full)\033[0m"
cargo +nightly udeps -q --lib --no-default-features --features full
