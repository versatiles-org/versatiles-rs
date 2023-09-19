#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo "check cargo fmt"
result=$(cargo fmt -- --check 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo fmt"
   exit 1
fi

echo "check cargo clippy "
result=$(cargo clippy --workspace --all-features -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo clippy"
   exit 1
fi

echo "check cargo test"
result=$(cargo test --workspace --all-features 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test bin"
   exit 1
fi

exit 0
