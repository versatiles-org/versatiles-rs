#!/usr/bin/env bash

cd "$(dirname "$0")/.."

mkdir -p target/llvm-cov
cargo llvm-cov test --bin versatiles --all-features --tests --lcov --output-path target/llvm-cov/lcov.info
cargo llvm-cov report --html
clear
cargo llvm-cov report
