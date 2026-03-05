#!/usr/bin/env bash
cd "$(dirname "$0")/.."

cargo build -F gdal

./target/debug/versatiles help --raw pipeline >versatiles_pipeline/README.md
./target/debug/versatiles help --raw config >versatiles/CONFIG.md
