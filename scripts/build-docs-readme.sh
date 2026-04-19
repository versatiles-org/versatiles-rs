#!/usr/bin/env bash
# Regenerate the pipeline and config reference READMEs from the built binary.
#
# Builds a debug binary with GDAL, then uses "versatiles help --raw" to
# overwrite versatiles_pipeline/README.md and versatiles/CONFIG.md with
# up-to-date Markdown output.

cd "$(dirname "$0")/.."

cargo build -F gdal

./target/debug/versatiles help --raw pipeline >versatiles_pipeline/README.md
./target/debug/versatiles help --raw config >versatiles/CONFIG.md
