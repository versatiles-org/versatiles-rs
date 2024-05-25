#!/usr/bin/env bash
cd "$(dirname "$0")/.."

mkdir -p target/llvm-cov
cargo llvm-cov test --bins --all-features --tests --lcov --output-path target/llvm-cov-target/lcov.info $1
cargo llvm-cov report
