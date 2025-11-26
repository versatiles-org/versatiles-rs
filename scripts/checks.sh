#!/usr/bin/env bash
cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

# Load GDAL environment variables
source scripts/env-gdal.sh

set +e

echo "cargo check"
result=$(cargo check --workspace --all-features --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi

echo "cargo fmt"
result=$(cargo fmt -- --check 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo fmt"
   exit 1
fi

echo "cargo clippy"
cd $PROJECT_DIR
result=$(cargo clippy --workspace --all-features --all-targets -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo clippy"
   exit 1
fi

echo "cargo test"
cd $PROJECT_DIR
result=$(cargo test 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test"
   exit 1
fi

echo "cargo test all features"
cd $PROJECT_DIR
result=$(cargo test --all-features 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test all features"
   exit 1
fi

echo "cargo doc"
cd $PROJECT_DIR
result=$(RUSTDOCFLAGS="-D warnings" cargo doc --no-deps 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo doc"
   exit 1
fi

exit 0
