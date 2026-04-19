#!/usr/bin/env bash
# Build the debug binary with GDAL support enabled.
#
# Requires GDAL development libraries. Install them first with:
#   ./scripts/install-gdal.sh

cd "$(dirname "$0")/.."

cargo build -F gdal
