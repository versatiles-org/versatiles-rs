#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# cargo install toml-cli

set -e

RED="\033[1;31m"
GRE="\033[1;32m"
END="\033[0m"

# Validate argument
VALID_ARGS="patch minor major alpha beta rc dev"
if [ -z "$1" ]; then
	echo "❗️ Need argument for bumping version: patch, minor, major, alpha, beta, rc, or dev"
	exit 1
fi

if ! echo "$VALID_ARGS" | grep -wq "$1"; then
	echo -e "${RED}❗️ Invalid argument: $1${END}"
	echo "Must be one of: $VALID_ARGS"
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

# Determine cargo-release command based on argument
RELEASE_ARG="$1"
case "$1" in
	dev)
		# dev requires custom pre-release-identifier
		echo "Releasing dev version..."
		cargo release --pre-release-identifier dev --no-verify --sign-commit --workspace --execute
		;;
	alpha|beta|rc)
		# cargo-release natively supports these
		echo "Releasing $RELEASE_ARG version..."
		cargo release "$RELEASE_ARG" --no-verify --sign-commit --workspace --execute
		;;
	patch|minor|major)
		# Existing stable release behavior
		echo "Releasing $RELEASE_ARG version..."
		cargo release "$RELEASE_ARG" --no-verify --sign-commit --workspace --execute
		;;
esac

# commit package.json if it was updated by cargo-release
if [ -n "$(git status --porcelain versatiles_node/package.json)" ]; then
	git add versatiles_node/package.json
	git commit --amend --no-edit --no-verify
fi
