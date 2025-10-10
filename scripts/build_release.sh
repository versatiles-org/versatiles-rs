#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# Load GDAL environment variables
source scripts/gdal-build-env.sh

cargo build -F gdal,bindgen --release
