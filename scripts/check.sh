#!/usr/bin/env bash
# Run all quality checks: Rust, Node.js, and Markdown.
#
# Delegates to check-rust.sh, check-node.sh, and check-markdown.sh in order.
# Run this before committing or opening a pull request.

cd "$(dirname "$0")/.."

set -e

./scripts/check-rust.sh
./scripts/check-node.sh
./scripts/check-markdown.sh

echo ""
echo "=========================================="
echo "All checks passed!"
echo "=========================================="

exit 0
