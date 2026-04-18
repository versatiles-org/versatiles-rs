#!/usr/bin/env bash

PROJECT_DIR="$(dirname "$0")/.."
# Skip e2e tests (test functions prefixed with e2e_) during coverage
cargo llvm-cov test --workspace --all-features --tests --lcov --output-path "$PROJECT_DIR/lcov.info" $1 -- --skip e2e_

cargo llvm-cov report --color always | awk '
{
   if (NR == 1) {
      end1   = index($0, "Regions")  - 1
      start2 = index($0, " Lines")   + 1
      end2   = index($0, "Branches") - 1
      offset1 = 0
      offset2 = 0
   }
   if (NR == 3) {
      offset1 = 33
      offset2 = 18
   }
   print substr($0, 1, end1) substr($0, start2 + offset1, end2 - start2 + 1 + offset2)
}'
