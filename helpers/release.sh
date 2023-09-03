#!/usr/bin/env bash

cd "$(dirname "$0")/.."

set -e

RED="\033[1;31m"
GRE="\033[1;32m"
END="\033[0m"

cargo check

# check if nothing to commit
if [ "$(git status --porcelain)" ]; then
	echo "❗️ Please commit all uncommitted changes!"
	exit 1
fi

# get versions
old_tag=$(curl -s "https://api.github.com/repos/versatiles-org/versatiles-rs/tags" | jq -r 'first(.[] | .name | select(startswith("v")))')
new_tag=v$(cat Cargo.toml | sed -ne 's/^version[ ="]*\([0-9\.]*\).*$/\1/p')

echo "old version: $old_tag"
echo "new version: $new_tag"

if [ $new_tag == $old_tag ]; then
	echo -e "${RED}New version $new_tag already exists!${END}"
	echo -e "${RED}ABORT${END}"
	exit 1
fi

# check cargo
./helpers/check.sh
if [ $? -ne 0 ]; then
	echo "❗️ Check failed!"
	exit 1
fi

# publish to crates.io
cargo publish --no-verify

# git tag
git tag -f -a "$new_tag" -m "new release: $new_tag"
git push --no-verify --follow-tags
