#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# Run all tests and check their duration

RUST_BACKTRACE=1 cargo +nightly test --bins --lib --all-features --workspace -- -Zunstable-options --report-time
