#!/usr/bin/env bash
# Lint all Markdown files in the repository with markdownlint-cli2.
#
# Checks all *.md files, excluding hidden directories, node_modules, and
# the build target directory. Uses npx to install markdownlint-cli2 on demand.

cd "$(dirname "$0")/.."

set +e

echo "=========================================="
echo "Markdown Checks"
echo "=========================================="

echo "markdownlint"
result=$(npx --yes markdownlint-cli2 "**/*.md" "#.**" "#versatiles_node/node_modules" "#target" 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: markdownlint"
   exit 1
fi

echo "Markdown checks passed!"
exit 0
