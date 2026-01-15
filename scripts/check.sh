#!/usr/bin/env bash
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
