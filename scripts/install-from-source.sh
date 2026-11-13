#!/usr/bin/env bash
# Build the latest stable release from source, with every feature, and install
# it with `cargo install` (into ~/.cargo/bin by default).
#
# Usage:
#   ./scripts/install-from-source.sh           # the latest stable release
#   ./scripts/install-from-source.sh v5.12.0   # a particular release
#
# Builds the release tag from GitHub, not the crate from crates.io: the `gdal`
# feature needs the GDAL bindings pinned in `versatiles_pipeline/Cargo.toml`,
# and a crate on crates.io falls back to the published ones, which do not build
# against GDAL 3.13. The checkout this script lives in is not used.
#
# Requires GDAL development libraries and libclang, for the `gdal` feature.
# Install them first with:
#   ./scripts/install-gdal.sh

set -euo pipefail

REPO="https://github.com/versatiles-org/versatiles-rs.git"

case "${1:-}" in
  -h|--help)
    echo "Usage: $(basename "$0") [tag]"
    echo "  tag  The release to build, e.g. v5.12.0. Defaults to the latest stable one."
    exit 0 ;;
esac
if [ "$#" -gt 1 ]; then
  echo "Expected at most one argument, the release tag" >&2
  exit 1
fi

if ! command -v gdal-config >/dev/null; then
  echo "GDAL is not installed (no gdal-config on PATH). Run ./scripts/install-gdal.sh first." >&2
  exit 1
fi

TAG="${1:-}"
if [ -z "$TAG" ]; then
  # Stable releases only: a tag like v5.0.0-rc.1 has a suffix and is skipped.
  TAG="$(git ls-remote --tags --refs "$REPO" 'v*' \
    | sed 's|.*refs/tags/||' \
    | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' \
    | sort -V \
    | tail -n 1)"
  if [ -z "$TAG" ]; then
    echo "Found no release tag in $REPO" >&2
    exit 1
  fi
fi

echo "Building versatiles $TAG with all features (GDAL $(gdal-config --version))"
# `--force` replaces a versatiles installed by cargo from elsewhere, such as
# crates.io, which cargo would otherwise refuse to overwrite.
cargo install --git "$REPO" --tag "$TAG" --locked --all-features --force versatiles

BINARY="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/versatiles"
"$BINARY" --version
# An install from install-unix.sh lives in /usr/local/bin and may come first.
FOUND="$(command -v versatiles || true)"
if [ "$FOUND" != "$BINARY" ]; then
  echo "Note: 'versatiles' on PATH is ${FOUND:-missing}, not $BINARY" >&2
fi
