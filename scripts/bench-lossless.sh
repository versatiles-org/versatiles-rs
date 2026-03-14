#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

echo "Running WebP lossless benchmark..."
cargo run --release --example webp_lossless_bench -p versatiles_image

echo ""
echo "Running PNG lossless benchmark..."
cargo run --release --example png_lossless_bench -p versatiles_image
