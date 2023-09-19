#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# cargo install toml-cli

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
ver_bin="v$(toml get -r versatiles/Cargo.toml package.version)"
ver_lib="v$(toml get -r versatiles-lib/Cargo.toml package.version)"
ver_lib_dep="v$(toml get -r versatiles/Cargo.toml dependencies.versatiles-lib.version)"

if [ $ver_bin != $ver_lib ]; then
	echo -e "${RED}The versions of lib ($ver_lib) and bin ($ver_bin) must be same!${END}"
	exit 1
fi

if [ $ver_lib != $ver_lib_dep ]; then
	echo -e "${RED}The versions of lib ($ver_lib) and the lib dependency in bin ($ver_lib_dep) must be same!${END}"
	exit 1
fi

new_tag=$ver_bin

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
cargo publish --package versatiles-lib --no-verify
cargo publish --package versatiles --no-verify

# git tag
git tag -f -a "$new_tag" -m "new release: $new_tag"
git push --no-verify --follow-tags
