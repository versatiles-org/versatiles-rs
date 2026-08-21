#!/usr/bin/env bash
# Regenerate the pipeline and config reference READMEs from the built binary.
#
# Builds a debug binary with GDAL, then uses "versatiles help --raw" to
# overwrite versatiles_pipeline/README.md and versatiles/CONFIG.md with
# up-to-date Markdown output.
#
# The output is formatted after being written. Without that, regenerating would
# leave these two files as the only unformatted Markdown in the repository, and
# check-markdown.sh would then fail depending on whether the generator or the
# formatter ran last.

cd "$(dirname "$0")/.."

cargo build -F gdal

./target/debug/versatiles help --raw pipeline >versatiles_pipeline/README.md
./target/debug/versatiles help --raw config >versatiles/CONFIG.md

npx --yes prettier --write versatiles_pipeline/README.md versatiles/CONFIG.md
