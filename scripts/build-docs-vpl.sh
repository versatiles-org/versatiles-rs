#!/usr/bin/env bash
cd "$(dirname "$0")/.."

cargo build
./target/debug/versatiles help --raw pipeline >versatiles_pipeline/README.md
