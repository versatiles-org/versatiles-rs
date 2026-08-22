#!/usr/bin/env bash
# Run all Rust quality checks across the workspace.
#
# Steps (in order): rustfmt, cargo check (no-default-features, server, cli,
# server+cli, default, all-features), clippy with -D warnings, tests with all
# features, doc build with -D warnings and the gdal feature, and the cargo-deny
# dependency policy.
#
# These mirror what CI checks, with one gap: CI also runs the test suite under
# the default and gdal feature sets, while this runs it only under
# --all-features. A green run here is therefore strong but not conclusive.
#
# Note that several steps use --all-features, so a local GDAL installation is
# required (scripts/install-gdal.sh).

cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

set +e

echo "=========================================="
echo "Rust Checks"
echo "=========================================="

echo "cargo fmt-all"
cargo fmt-all

echo "cargo check"
result=$(cargo check --color=always --workspace --no-default-features --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - server"
result=$(cargo check --color=always --workspace --no-default-features --features server --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - cli"
result=$(cargo check --color=always --workspace --no-default-features --features cli --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - server, cli"
result=$(cargo check --color=always --workspace --no-default-features --features server,cli --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
# The feature set released binaries are built with. Bracketed by the
# server,cli and all-features checks above and below, but not implied by
# either: `default` also pulls in ssh2.
echo "cargo check - default features"
result=$(cargo check --color=always --workspace --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi
echo "cargo check - all features"
result=$(cargo check --color=always --workspace --all-features --all-targets 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo check"
   exit 1
fi

echo "cargo clippy"
cd $PROJECT_DIR
result=$(cargo clippy --color=always --workspace --all-features --all-targets -- -D warnings 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo clippy"
   exit 1
fi

# echo "cargo test"
# cd $PROJECT_DIR
# result=$(cargo test --color=always 2>&1)
# if [ $? -ne 0 ]; then
#    echo -e "$result\nERROR DURING: cargo test"
#    exit 1
# fi

echo "cargo test all features"
cd $PROJECT_DIR
result=$(cargo test --color=always --all-features 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo test all features"
   exit 1
fi

# Matches the CI docs job exactly, `--features gdal` included: a broken
# intra-doc link inside a gdal-gated item is invisible without it, and CI
# treats every rustdoc warning as an error.
echo "cargo doc"
cd $PROJECT_DIR
result=$(RUSTDOCFLAGS="-D warnings" cargo doc --color=always --no-deps --features gdal 2>&1)
if [ $? -ne 0 ]; then
   echo -e "$result\nERROR DURING: cargo doc"
   exit 1
fi

# Dependency policy: RustSec advisories, the licence allow-list, and the
# crates.io-only source rule. Mirrors the "Linux: Dependencies" CI job; the
# policy and its exceptions live in deny.toml.
#
# Skipped rather than failed when cargo-deny is missing: it is a separate
# binary rather than part of the toolchain, so a contributor without it should
# still get a usable check run. CI always has it.
cd $PROJECT_DIR
if command -v cargo-deny >/dev/null 2>&1; then
   echo "cargo deny"
   result=$(cargo deny check 2>&1)
   if [ $? -ne 0 ]; then
      echo -e "$result\nERROR DURING: cargo deny"
      exit 1
   fi
else
   echo "cargo deny (skipped: not installed — brew install cargo-deny)"
fi

echo "Rust checks passed!"
exit 0
