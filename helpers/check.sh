#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

echo "check cargo fmt"
result=$(cargo fmt --all -- --check 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo fmt"
   exit 1
fi

echo "check cargo clippy "
result=$(cargo clippy --all -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo clippy"
   exit 1
fi

echo "check cargo test"
result=$(cargo test --workspace 2>&1)
if [ $? -ne 0 ]; then
   echo "$result"
   echo "ERROR DURING: cargo test"
   exit 1
fi

exit 0
