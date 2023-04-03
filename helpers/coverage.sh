#!/usr/bin/env bash
cd "$(dirname "$0")"
cd ..

# prepare
rm -rf coverage
mkdir coverage

# generate test coverage
mkdir coverage/profraw
CARGO_INCREMENTAL=0 RUSTFLAGS='-Cinstrument-coverage' LLVM_PROFILE_FILE='./coverage/profraw/cargo-test-%p-%m.profraw' cargo test --workspace

# generate html
grcov coverage/profraw/. --binary-path ./target/debug/deps/ -s . -t html --branch --ignore-not-existing --ignore '../*' --ignore "/*" -o ./coverage/html

# generate lcov
grcov coverage/profraw/. --binary-path ./target/debug/deps/ -s . -t lcov --branch --ignore-not-existing --ignore '../*' --ignore "/*" -o ./coverage/tests.lcov
