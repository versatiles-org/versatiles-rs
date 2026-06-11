#!/usr/bin/env bash
# CI script: create a draft GitHub release for the latest version tag.
#
# Fetches the two most recent version tags via the GitHub API, assembles a
# grouped changelog from the commits between them with git-cliff, and creates (or
# updates) a draft pre-release. Writes the tag name to GITHUB_OUTPUT and the notes
# to GITHUB_STEP_SUMMARY for use in subsequent CI steps.
#
# Grouping/filtering is configured in cliff.toml (grouped by Conventional-Commit
# type, noise dropped). A "Full changelog" compare link is appended. The result is
# a starting point: the release stays a DRAFT so a maintainer can add a summary /
# highlights / breaking notes before publishing.
#
# Requires `git-cliff` on PATH (installed by the Release workflow's prepare job) and
# a full-history checkout (fetch-depth: 0) so the tag range is available locally.

cd "$(dirname "$0")/.."

set -eo pipefail

REPO="versatiles-org/versatiles-rs"

if ! command -v git-cliff >/dev/null 2>&1; then
  echo "git-cliff is required but was not found on PATH." >&2
  echo "Install it in the workflow (taiki-e/install-action with tool: git-cliff)." >&2
  exit 1
fi

# Get latest tags using gh CLI
gh api "repos/$REPO/tags" --paginate >tags.json

# Check if we got valid JSON
if ! jq -e . >/dev/null 2>&1 <tags.json; then
  echo "Failed to fetch tags from GitHub API. Response:" >&2
  cat tags.json >&2
  exit 1
fi

# get new tag (latest) and old tag (previous), keeping the existing ordering
NEW_TAG=$(jq -r "nth(0; .[] | .name | select(startswith(\"v\")))" tags.json)
OLD_TAG=$(jq -r "nth(1; .[] | .name | select(startswith(\"v\")))" tags.json)
export NEW_TAG
rm -f tags.json

# get version via cargo.toml
VERSION=$(sed -n "s/^version *= *\"\(.*\)\"/v\1/p" ./Cargo.toml | tr -d '\n')

# compare versions
if [ "$NEW_TAG" != "$VERSION" ]; then
  echo "Current cargo version ($VERSION) is not latest tag ($NEW_TAG)" >&2
  exit 1
fi

# Assemble grouped release notes for the commits in OLD_TAG..NEW_TAG with git-cliff
# (grouping/filtering configured in cliff.toml), then append a compare link.
{
  # `--strip all` drops the (empty) header/footer; the sed removes any leading
  # blank lines git-cliff emits before the first group.
  git-cliff --config cliff.toml --strip all "$OLD_TAG..$NEW_TAG" | sed '/./,$!d'
  echo
  echo "**Full changelog:** https://github.com/$REPO/compare/$OLD_TAG...$NEW_TAG"
} >notes.txt

# Try to create release (keeps existing drafts untouched on re-run)
gh release view "$NEW_TAG" || gh release create "$NEW_TAG" --title "$NEW_TAG" -F notes.txt --draft --prerelease

# return results to GitHub (no-op when run locally)
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "tag=$NEW_TAG" >>"$GITHUB_OUTPUT"
fi
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  cat notes.txt >>"$GITHUB_STEP_SUMMARY"
fi
