#!/usr/bin/env bash
# Developer test script: format, lint, and test the Rust workspace on Unix.
#
# Runs rustfmt, clippy (binary + lib with multiple feature combinations),
# and cargo test (bins, lib, doc tests) with colored output. Accepts an
# optional extra cargo argument (e.g. -- --nocapture) forwarded to each step.

cd "$(dirname "$0")/.."

set -e

# Set environment variable
RUST_BACKTRACE=1

# Format
echo -e "\033[1;33mFormatting...\033[0m"
cargo fmt

# Clippy
echo -e "\033[1;33mRunning clippy for binary...\033[0m"
cargo clippy --quiet --bin versatiles --all-features --all-targets $1

echo -e "\033[1;33mRunning clippy for library...\033[0m"
cargo clippy --quiet --lib --no-default-features --all-targets $1

echo -e "\033[1;33mRunning clippy for library (big)...\033[0m"
cargo clippy --quiet --lib --all-features --all-targets $1

# Test
echo -e "\033[1;33mRunning tests for binary...\033[0m"
cargo test --quiet --bins --all-features $1

echo -e "\033[1;33mRunning tests for library...\033[0m"
cargo test --quiet --lib --no-default-features $1

echo -e "\033[1;33mRunning tests for library (big)...\033[0m"
cargo test --quiet --lib --all-features $1

echo -e "\033[1;33mRunning doc tests (big)...\033[0m"
cargo test --quiet --doc --all-features $1
