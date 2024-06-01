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

cargo check

# check cargo
./helpers/test.sh
if [ $? -ne 0 ]; then
	echo "❗️ Check failed!"
	exit 1
fi

# publish to crates.io
cargo release "$1"
