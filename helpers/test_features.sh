#!/usr/bin/env bash

echo -e "\033[1;30mtest bin\033[0m"
cargo test --quiet --bin versatiles --all-features

echo -e "\033[1;30mtest lib\033[0m"
cargo test --quiet --lib

exit 0
