#!/usr/bin/env bash
cd "$(dirname "$0")/.."

source scripts/gdal-build-env.sh
cargo build -F gdal,bindgen --release
