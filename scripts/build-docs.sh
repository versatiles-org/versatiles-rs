#!/usr/bin/env bash
cd "$(dirname "$0")/.."

rm -rf doc
cargo doc --no-deps
