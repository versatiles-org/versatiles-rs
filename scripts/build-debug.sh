#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# Load GDAL environment variables
source scripts/env-gdal.sh

cargo build -F gdal,bindgen
