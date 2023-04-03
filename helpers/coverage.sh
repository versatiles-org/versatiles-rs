#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

cargo llvm-cov test --workspace --tests --lcov --output-path ./lcov.info
