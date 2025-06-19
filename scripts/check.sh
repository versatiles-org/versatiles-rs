#!/usr/bin/env bash
cd "$(dirname "$0")/.."

PROJECT_DIR=$(pwd)

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

echo "cargo clippy:"

run_clippy() {
   cd "${PROJECT_DIR}$1"
   echo "  $1"
   result=$(cargo clippy --all-features --all-targets -- -D warnings 2>&1)
   if [ $? -ne 0 ]; then
      echo -e "$result\nERROR DURING: cargo clippy $1"
      exit 1
   fi
}

run_clippy /
run_clippy /versatiles
run_clippy /versatiles_container
run_clippy /versatiles_core
run_clippy /versatiles_derive
run_clippy /versatiles_geometry
run_clippy /versatiles_image
run_clippy /versatiles_pipeline
cd $PROJECT_DIR


echo "cargo test:"

run_test() {
   cd "${PROJECT_DIR}$1"
   echo "  $1"
   result=$(cargo test --all-features 2>&1)
   if [ $? -ne 0 ]; then
      echo -e "$result\nERROR DURING: cargo test $1"
      exit 1
   fi
}

run_test /
run_test /versatiles
run_test /versatiles_container
run_test /versatiles_core
run_test /versatiles_derive
run_test /versatiles_geometry
run_test /versatiles_image
run_test /versatiles_pipeline

exit 0
