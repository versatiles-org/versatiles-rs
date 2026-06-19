#!/usr/bin/env bash
# Auto-format the codebase in place: Rust, Markdown, and Node.js.
#
# The write-mode counterpart to check.sh. Steps:
#   - Rust:     cargo fmt
#   - Markdown: markdownlint-cli2 --fix (same file set as check-markdown.sh)
#   - Node.js:  npm run fix (eslint --fix + prettier --write) in versatiles_node
#
# Linters can only auto-fix some issues; anything left needs manual attention.
# This script reports such leftovers and exits non-zero, but always runs every
# formatter first so a failure in one does not block the others.

cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

set +e
FAILED=""

echo "=========================================="
echo "Formatting Rust"
echo "=========================================="
cargo fmt
if [ $? -ne 0 ]; then
   FAILED="${FAILED} rust"
fi

echo ""
echo "=========================================="
echo "Formatting Markdown"
echo "=========================================="
npx --yes markdownlint-cli2 --fix "**/*.md" "#.**" "#versatiles_node/node_modules" "#target"
if [ $? -ne 0 ]; then
   echo "Some Markdown issues could not be auto-fixed (see above)."
   FAILED="${FAILED} markdown"
fi

echo ""
echo "=========================================="
echo "Formatting Node.js (versatiles_node)"
echo "=========================================="
NODEJS_DIR="${PROJECT_DIR}/versatiles_node"
if [ ! -d "$NODEJS_DIR" ]; then
   echo "Node.js directory not found, skipping Node.js formatting"
else
   cd "$NODEJS_DIR"
   if [ ! -d "node_modules" ]; then
      echo "Installing Node.js dependencies..."
      npm install || FAILED="${FAILED} node-install"
   fi
   npm run fix
   if [ $? -ne 0 ]; then
      echo "Some Node.js issues could not be auto-fixed (see above)."
      FAILED="${FAILED} node"
   fi
   cd "$PROJECT_DIR"
fi

echo ""
echo "=========================================="
if [ -n "$FAILED" ]; then
   echo "Formatting finished with leftovers needing manual fixes:${FAILED}"
   echo "=========================================="
   exit 1
fi
echo "Formatting complete!"
echo "=========================================="
exit 0
