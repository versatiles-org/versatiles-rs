#!/usr/bin/env bash
cd "$(dirname "$0")/.."

echo -e "\033[1;30mfind unused dependencies033[0m"
cargo +nightly udeps --quiet --workspace
