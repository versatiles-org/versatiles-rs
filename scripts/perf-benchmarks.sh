#!/usr/bin/env bash
# Run all unit tests with per-test timing via libtest's --report-time flag.
#
# Uses the nightly toolchain for the -Zunstable-options flag. For a richer
# timing analysis with ranking and module summaries, use test-timing.sh instead.

cd "$(dirname "$0")/.."

RUST_BACKTRACE=1 cargo +nightly test --bins --lib --all-features --workspace -- -Zunstable-options --report-time
