#!/usr/bin/env bash
cd "$(dirname "$0")/.."

source scripts/env-gdal.sh
cargo build -F gdal,bindgen

./target/debug/versatiles help --raw pipeline >versatiles_pipeline/README.md
./target/debug/versatiles help --raw config >versatiles/CONFIG.md
