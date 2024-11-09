#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo "cargo check"
result=$(cargo check 2>&1)
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

echo "cargo clippy "
result=$(cargo clippy --workspace --all-features -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo clippy"
   exit 1
fi

#echo "cargo test library"
#result=$(cargo test --lib --all-features 2>&1)
#if [ $? -ne 0 ]; then
#   echo -e "$result\nERROR DURING: cargo test bin"
#   exit 1
#fi

echo "cargo test binary"
result=$(cargo test --bins --all-features 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test bin"
   exit 1
fi

exit 0
