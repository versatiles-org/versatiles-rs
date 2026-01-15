#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set +e

echo "=========================================="
echo "Markdown Checks"
echo "=========================================="

echo "markdownlint"
result=$(npx markdownlint-cli2 "**/*.md" "#node_modules" "#versatiles_node/node_modules" "#target" 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: markdownlint"
   exit 1
fi

echo "Markdown checks passed!"
exit 0
