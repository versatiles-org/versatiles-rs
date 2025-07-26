#!/usr/bin/env bash

mkdir -p "$(dirname "$0")/../target/llvm-cov"
cargo llvm-cov test --bins --all-features --tests --lcov $1
cargo llvm-cov report
