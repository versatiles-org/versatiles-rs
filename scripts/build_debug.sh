#!/usr/bin/env bash
cd "$(dirname "$0")/.."

cargo build -F gdal,bindgen
