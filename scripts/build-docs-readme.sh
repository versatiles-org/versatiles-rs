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
#
# It is formatted with the Prettier version `versatiles_node/package-lock.json`
# pins, the one CI's format check uses. CI also runs this script and fails if
# the files differ from the committed ones, which only works if both format
# alike.

set -e
cd "$(dirname "$0")/.."

cargo build -F gdal

./target/debug/versatiles help --raw pipeline >versatiles_pipeline/README.md
./target/debug/versatiles help --raw config >versatiles/CONFIG.md

PRETTIER_VERSION=$(node -p "require('./versatiles_node/package-lock.json').packages['node_modules/prettier'].version")
npx --yes "prettier@$PRETTIER_VERSION" --write versatiles_pipeline/README.md versatiles/CONFIG.md
