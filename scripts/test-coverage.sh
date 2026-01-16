#!/usr/bin/env bash

PROJECT_DIR="$(dirname "$0")/.."
source "$PROJECT_DIR/scripts/env-gdal.sh"
# Skip e2e tests (test functions prefixed with e2e_) during coverage
cargo llvm-cov test --bins --all-features --tests --lcov --output-path "$PROJECT_DIR/lcov.info" $1 -- --skip e2e_
cargo llvm-cov report
