#!/usr/bin/env bash

cd "$(dirname "$0")/.."

echo "check cargo fmt"
result=$(cargo fmt -- --check 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo fmt"
   exit 1
fi

echo "check cargo clippy "
result=$(cargo clippy -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo clippy"
   exit 1
fi

echo "check cargo test lib"
result=$(cargo test --lib 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo test lib"
   exit 1
fi

echo "check cargo test bin"
result=$(cargo test --bin versatiles 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo test bin"
   exit 1
fi

exit 0
