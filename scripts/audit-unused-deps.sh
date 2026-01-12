#!/usr/bin/env bash
cd "$(dirname "$0")/.."
set -e

# Features by package:
# cli:    versatiles, versatiles_container, versatiles_core
# server: versatiles
# test:   versatiles_container, versatiles_core, versatiles_geometry, versatiles_image
# gdal:   versatiles_pipeline

RED="\033[1;31m"
GRE="\033[1;32m"
END="\033[0m"

# Check prerequisites
if ! rustup toolchain list | grep -q nightly; then
	echo "Installing nightly toolchain..."
	rustup toolchain install nightly
fi

if ! cargo +nightly udeps --version &>/dev/null; then
	echo -e "${RED}cargo-udeps not found. Install with: cargo install cargo-udeps${END}"
	exit 1
fi

FAILED=0

run_check() {
	local name="$1"
	shift
	printf "%-30s" "$name"
	if cargo +nightly udeps -q "$@" 2>&1; then
		echo -e "${GRE}OK${END}"
	else
		echo -e "${RED}FAILED${END}"
		FAILED=$((FAILED + 1))
	fi
}

run_check "Binary targets" --bins
run_check "Workspace: no features" --lib --workspace --no-default-features
run_check "Workspace: defaults" --lib --workspace
run_check "Feature: cli" --lib --workspace --no-default-features --features cli --exclude versatiles_derive --exclude versatiles_geometry --exclude versatiles_image --exclude versatiles_node --exclude versatiles_pipeline
run_check "Feature: server" --lib --package versatiles --no-default-features --features server
run_check "Feature: test" --lib --workspace --no-default-features --features test --exclude versatiles --exclude versatiles_derive --exclude versatiles_node --exclude versatiles_pipeline
run_check "Feature: gdal" --lib --package versatiles_pipeline --no-default-features --features gdal
run_check "All features" --lib --workspace --all-features --exclude versatiles --exclude versatiles_core

if [ $FAILED -gt 0 ]; then
	echo -e "\n${RED}$FAILED check(s) failed${END}"
	exit 1
fi
