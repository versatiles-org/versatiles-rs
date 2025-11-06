#!/usr/bin/env bash
set -euo pipefail

# Determine important paths
SCRIPT_DIR="$(cd -- "$(dirname "$0")" >/dev/null 2>&1; pwd -P)"
PROJECT_DIR="$(cd -- "$SCRIPT_DIR/.." >/dev/null 2>&1; pwd -P)"
CURRENT_DIR="$(pwd -P)"

# List of workspace packages we care about (directory names relative to project root)
packages=(
  "versatiles"
  "versatiles_container"
  "versatiles_core"
  "versatiles_derive"
  "versatiles_geometry"
  "versatiles_image"
  "versatiles_pipeline"
)

# Decide which packages to run based on where the script is invoked from.
# If CURRENT_DIR is inside one of the package directories, run only for that one.
selected_packages=()
for pkg in "${packages[@]}"; do
  pkg_dir="$PROJECT_DIR/$pkg"

  if [[ "$CURRENT_DIR" == "$pkg_dir" ]]; then
    selected_packages+=("$pkg")
    break
  fi
done

# If none matched, run for all packages
if [[ ${#selected_packages[@]} -eq 0 ]]; then
  selected_packages=("${packages[@]}")
fi

# Run coverage for the selected packages
for package in "${selected_packages[@]}"; do
  pkg_path="$PROJECT_DIR/$package"
  echo "=== $package ==="
  (cd "$pkg_path" && cargo +nightly rustdoc -- -Z unstable-options --show-coverage 2>/dev/null)
  echo
done
