#!/usr/bin/env bash
cd "$(dirname "$0")/.."

set -e

RED="\033[1;31m"
GRE="\033[1;32m"
END="\033[0m"

cargo check --workspace

# check if nothing to commit
if [ -n "$(git status --porcelain)" ]; then
	echo "❗️ Please commit all uncommitted changes!"
	exit 1
fi

# get versions
old_tag=$(curl -s "https://api.github.com/repos/versatiles-org/versatiles-rs/tags" | jq -r 'first(.[] | .name | select(startswith("v")))')
tag_bin=v$(cat versatiles/Cargo.toml | sed -ne 's/^version[ ="]*\([0-9\.]*\).*$/\1/p')
tag_lib=v$(cat versatiles_lib/Cargo.toml | sed -ne 's/^version[ ="]*\([0-9\.]*\).*$/\1/p')

echo "old version: $old_tag"
echo "version bin: $tag_bin"
echo "version lib: $tag_lib"

if [ $tag_bin != $tag_lib ]; then
	echo -e "${RED}The versions of lib ($tag_lib) and bin ($tag_bin) must be same!${END}"
	exit 1
fi

new_tag=$tag_bin

if [ $new_tag == $old_tag ]; then
	echo -e "${RED}New version $new_tag already exists!${END}"
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
