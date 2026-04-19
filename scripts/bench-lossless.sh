#!/usr/bin/env bash
# Run lossless compression benchmarks for WebP and PNG image formats.
#
# Executes example binaries from the versatiles_image crate to measure
# encoding performance of lossless WebP and PNG codecs.

cd "$(dirname "$0")/.."

set -e

echo "Running WebP lossless benchmark..."
cargo run --release --example webp_lossless_bench -p versatiles_image

echo ""
echo "Running PNG lossless benchmark..."
cargo run --release --example png_lossless_bench -p versatiles_image
