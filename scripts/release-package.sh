#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# cargo install toml-cli

set -e

RED="\033[1;31m"
GRE="\033[1;32m"
END="\033[0m"

if [ -z "$1" ]; then
	echo "❗️ Need argument for bumping version: \"patch\", \"minor\" or \"major\""
	exit 1
fi

# build readme docs
./scripts/build-docs-readme.sh

# check version synchronization
echo "Checking version synchronization..."
./scripts/sync-version.sh --fix

# check if git is clean
if [ -n "$(git status --porcelain)" ]; then
	echo -e "${RED}❗️ Git is not clean!${END}"
	git status --porcelain
	exit 1
fi

cargo check

# check cargo
./scripts/test-unix.sh
if [ $? -ne 0 ]; then
	echo "❗️ Check failed!"
	exit 1
fi

# execute the release
cargo release "$1" --no-verify --sign-commit --workspace --execute

# commit package.json if it was updated by cargo-release
if [ -n "$(git status --porcelain versatiles_node/package.json)" ]; then
	git add versatiles_node/package.json
	git commit --amend --no-edit --no-verify
fi
