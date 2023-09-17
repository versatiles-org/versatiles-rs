#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

# get latest tags
curl -s https://api.github.com/repos/versatiles-org/versatiles-rs/tags >tags.json

# get new tag
export NEW_TAG=$(jq -r "nth(0; .[] | .name | select(startswith(\"v\")))" tags.json)
# get old tag
OLD_TAG=$(jq -r "nth(1; .[] | .name | select(startswith(\"v\")))" tags.json)
# get old SHA
OLD_SHA=$(jq -r ".[] | select(.name == \"$OLD_TAG\") | .commit.sha" tags.json)

# get version via cargo.toml
VERSION=$(cat Cargo.toml | sed -n "s/^version *= *\"\(.*\)\"/v\1/p" | tr -d '\n')

# compare versions
if [ "$NEW_TAG" != "$VERSION" ]; then
   echo "Current cargo version ($VERSION) is not latest tag ($NEW_TAG)" >&2
   exit 1
fi

echo "# new release: $NEW_TAG" >notes.txt

curl -s "https://api.github.com/repos/versatiles-org/versatiles-rs/commits?per_page=100" |
   jq -r ".[] | if .sha == \"$OLD_SHA\" then halt else \"- \" + .commit.message end" |
   tac >>notes.txt

# Try to create release
gh release view "$NEW_TAG" || gh release create "$NEW_TAG" --title "$NEW_TAG" -F notes.txt --draft --prerelease

# return results to GitHub
echo "tag=$NEW_TAG" >>$GITHUB_OUTPUT
cat notes.txt >>$GITHUB_STEP_SUMMARY
