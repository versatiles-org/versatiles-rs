#!/usr/bin/env bash

echo -e "\033[1;30mtest bin\033[0m"
cargo +nightly udeps --quiet --bin versatiles

echo -e "\033[1;30mtest lib\033[0m"
cargo +nightly udeps --quiet --lib --no-default-features
