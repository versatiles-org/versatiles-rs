#!/usr/bin/env bash
# Generate Rust API documentation with cargo doc.
#
# Clears any previously generated docs in ./doc, then builds fresh HTML
# documentation for all workspace crates (excluding dependencies).

cd "$(dirname "$0")/.."

rm -rf doc
cargo doc --no-deps
