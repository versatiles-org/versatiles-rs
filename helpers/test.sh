#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

echo -e "\033[1;33mFormatting...\033[0m"
cargo fmt

echo -e "\033[1;33mRunning clippy for binary...\033[0m"
cargo clippy --quiet --bins $1

echo -e "\033[1;33mRunning clippy for library...\033[0m"
cargo clippy --quiet --lib --no-default-features $1

echo -e "\033[1;33mRunning clippy for library (http)...\033[0m"
cargo clippy --quiet --lib --no-default-features -F http $1

echo -e "\033[1;33mRunning clippy for library (full)...\033[0m"
cargo clippy --quiet --lib --no-default-features -F full $1

echo -e "\033[1;33mRunning tests for binary...\033[0m"
cargo test --quiet --bins $1

echo -e "\033[1;33mRunning tests for library...\033[0m"
cargo test --quiet --lib --no-default-features $1

echo -e "\033[1;33mRunning tests for library (http)...\033[0m"
cargo test --quiet --lib --no-default-features -F http $1

echo -e "\033[1;33mRunning tests for library (full)...\033[0m"
cargo test --quiet --lib --no-default-features -F full $1
