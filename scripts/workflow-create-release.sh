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

# The tags endpoint returns newest first (chronologically, not lexically — it
# lists v4.10.0 ahead of v4.9.1), so the release being built is the first one.
NEW_TAG=$(jq -r 'first(.[] | .name | select(startswith("v")))' tags.json)

# The changelog range ends at the previous *stable* tag, skipping prereleases.
#
# Taking simply the previous tag broke the release that matters most: promoting
# v5.0.0-rc.1 to v5.0.0 would span rc.1..5.0.0, which holds nothing but the
# version bump, so the notes for the actual major release came out empty. Every
# release now reports what changed since the last stable one — an rc shows the
# full scope a tester needs, and the final release repeats it for everyone who
# skipped the rc.
#
# A prerelease is any tag with a hyphen, per semver.
OLD_TAG=$(jq -r --arg new "$NEW_TAG" '
  first(.[] | .name
    | select(startswith("v"))
    | select(. != $new)
    | select(contains("-") | not))
' tags.json)

# No stable predecessor (a first release, or a history of prereleases only):
# fall back to whatever came before, and let git-cliff walk from the beginning
# if there is nothing at all.
if [ -z "$OLD_TAG" ] || [ "$OLD_TAG" = "null" ]; then
  OLD_TAG=$(jq -r --arg new "$NEW_TAG" \
    'first(.[] | .name | select(startswith("v")) | select(. != $new)) // empty' tags.json)
fi

export NEW_TAG
rm -f tags.json

# get version via cargo.toml
VERSION=$(sed -n "s/^version *= *\"\(.*\)\"/v\1/p" ./Cargo.toml | tr -d '\n')

# compare versions
if [ "$NEW_TAG" != "$VERSION" ]; then
  echo "Current cargo version ($VERSION) is not latest tag ($NEW_TAG)" >&2
  exit 1
fi

# Assemble grouped release notes for the commits since OLD_TAG with git-cliff
# (grouping/filtering configured in cliff.toml), then append a compare link.
#
# With no predecessor — a first release — there is no range to bound and no two
# points to compare, so walk the whole history and drop the link.
{
  # `--strip all` drops the (empty) header/footer; the sed removes any leading
  # blank lines git-cliff emits before the first group.
  if [ -n "$OLD_TAG" ]; then
    git-cliff --config cliff.toml --strip all "$OLD_TAG..$NEW_TAG" | sed '/./,$!d'
    echo
    echo "**Full changelog:** https://github.com/$REPO/compare/$OLD_TAG...$NEW_TAG"
  else
    git-cliff --config cliff.toml --strip all "$NEW_TAG" | sed '/./,$!d'
  fi
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
