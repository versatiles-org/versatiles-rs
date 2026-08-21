#!/usr/bin/env bash
# Reclaim disk space in target/ without discarding what you are still building.
#
# Cargo never garbage-collects target/: every configuration ever built stays
# there. This workspace produces a lot of them — six feature sets across the
# check matrix, dev and test profiles, a --target tree per release build, a
# complete second tree under llvm-cov-target/, plus every dependency version
# that predates a `cargo update`. One `cargo test --no-run` is around 3.6 GB, so
# the total is that figure times however many distinct builds have accumulated.
#
# Passes, in order:
#   1. artifacts from toolchains no longer installed (always dead)
#   2. the llvm-cov tree, which nothing else removes
#   3. artifacts not *accessed* in DAYS days — what you are working on survives
#
# Usage:
#   ./scripts/clean-target.sh            # keep anything used in the last 14 days
#   ./scripts/clean-target.sh 30         # ...or a period of your choosing
#   ./scripts/clean-target.sh --all      # remove target/ entirely (cargo clean)
#
# Requires cargo-sweep for the day-based pass:
#   cargo install cargo-sweep

cd "$(dirname "$0")/.."

set -e

DAYS="${1:-14}"

size_of() {
	# Reports 0 rather than failing when the directory is absent.
	[ -d "$1" ] && du -sk "$1" 2>/dev/null | cut -f1 || echo 0
}

human() {
	# GNU and BSD numfmt differ; do it in awk so this works on both.
	awk -v k="$1" 'BEGIN { printf (k > 1048576) ? "%.1f GB" : "%.0f MB", (k > 1048576) ? k/1048576 : k/1024 }'
}

BEFORE=$(size_of target)

echo "=========================================="
echo "Cleaning target/  (currently $(human "$BEFORE"))"
echo "=========================================="

if [ "$DAYS" = "--all" ]; then
	echo "cargo clean"
	cargo clean
else
	if command -v cargo-sweep >/dev/null 2>&1; then
		echo "cargo sweep --installed   (artifacts from removed toolchains)"
		cargo sweep --installed
	else
		echo "cargo-sweep not found — skipping the toolchain and age passes."
		echo "  install it with: cargo install cargo-sweep"
	fi

	if [ -d target/llvm-cov-target ]; then
		echo "removing target/llvm-cov-target   ($(human "$(size_of target/llvm-cov-target)"))"
		rm -rf target/llvm-cov-target
	fi

	if command -v cargo-sweep >/dev/null 2>&1; then
		echo "cargo sweep --time $DAYS   (artifacts unused for $DAYS days)"
		cargo sweep --time "$DAYS"
	fi
fi

AFTER=$(size_of target)

echo ""
echo "=========================================="
echo "  before: $(human "$BEFORE")"
echo "  after:  $(human "$AFTER")"
echo "  freed:  $(human "$((BEFORE - AFTER))")"
echo "=========================================="
