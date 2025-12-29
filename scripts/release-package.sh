#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# cargo install toml-cli

set -e

RED="\033[1;31m"
GRE="\033[1;32m"
YEL="\033[1;33m"
BLU="\033[1;34m"
END="\033[0m"

# Validate argument or provide interactive selection
VALID_ARGS="patch minor major alpha beta rc"
RELEASE_ARG=""

if [ -z "$1" ]; then
	echo -e "${BLU}Select release type:${END}"
	echo ""

	PS3=$'\nEnter selection number: '
	options=(
		"patch   - Bug fixes, small improvements (x.y.Z)"
		"minor   - New features, backward compatible (x.Y.0)"
		"major   - Breaking changes (X.0.0)"
		"alpha   - Early development, unstable API (x.y.z-alpha.N)"
		"beta    - Feature complete, testing phase (x.y.z-beta.N)"
		"rc      - Release candidate, final testing (x.y.z-rc.N)"
		"Cancel"
	)

	select opt in "${options[@]}"; do
		case $REPLY in
			1) RELEASE_ARG="patch"; break;;
			2) RELEASE_ARG="minor"; break;;
			3) RELEASE_ARG="major"; break;;
			4) RELEASE_ARG="alpha"; break;;
			5) RELEASE_ARG="beta"; break;;
			6) RELEASE_ARG="rc"; break;;
			7) echo -e "${YEL}Cancelled${END}"; exit 0;;
			*) echo -e "${RED}Invalid selection${END}";;
		esac
	done

	echo ""
	echo -e "${GRE}Selected: $RELEASE_ARG${END}"
	echo ""
else
	RELEASE_ARG="$1"

	if ! echo "$VALID_ARGS" | grep -wq "$RELEASE_ARG"; then
		echo -e "${RED}❗️ Invalid argument: $RELEASE_ARG${END}"
		echo "Must be one of: $VALID_ARGS"
		exit 1
	fi
fi

# build readme docs
./scripts/build-docs-readme.sh

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

# Perform the release
echo "Releasing $RELEASE_ARG version..."
cargo release "$RELEASE_ARG" --no-verify --sign-commit --workspace --execute --no-push

# sync version
echo "Checking version synchronization..."
./scripts/sync-version.sh --fix

# Amend the cargo-release commit to include package.json
if [ -n "$(git status --porcelain versatiles_node/package.json)" ]; then
	git add versatiles_node/package*
	git commit --amend --no-edit --no-verify
	echo -e "${GRE}✓ Package.json synced and commit amended${END}"
fi

git push origin main --follow-tags
