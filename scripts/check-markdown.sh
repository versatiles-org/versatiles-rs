#!/usr/bin/env bash
# Lint and check the formatting of every Markdown file in the repository.
#
# markdownlint-cli2 checks structure; prettier checks layout, which markdownlint
# does not touch — table columns above all, where hand-alignment drifts and three
# different styles had accumulated. Both run over all *.md files, excluding
# hidden directories, node_modules, and the build target directory, and both are
# fetched on demand with npx.
#
# Run scripts/format.sh to fix what either reports.

cd "$(dirname "$0")/.."

set +e

echo "=========================================="
echo "Markdown Checks"
echo "=========================================="

echo "markdownlint"
result=$(npx --yes markdownlint-cli2 "**/*.md" "#.**" "#versatiles_node/node_modules" "#target" --fix 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: markdownlint"
   exit 1
fi

echo "prettier"
result=$(npx --yes prettier --check "**/*.md" 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: prettier"
   exit 1
fi

echo "Markdown checks passed!"
exit 0
