#!/usr/bin/env bash
# Export GDAL environment variables for Rust compilation
set -euo pipefail

: "${WORKSPACE:=$(pwd)}"
GDAL_PREFIX="${WORKSPACE}/.toolchain/gdal"

# Core GDAL environment
export GDAL_PREFIX
export GDAL_CONFIG="${GDAL_PREFIX}/bin/gdal-config"
export GDAL_DATA="${GDAL_PREFIX}/share/gdal"
export GDAL_INCLUDE_DIR="${GDAL_PREFIX}/include"
export GDAL_LIB_DIR="${GDAL_PREFIX}/lib"
export PKG_CONFIG_PATH="${GDAL_PREFIX}/lib/pkgconfig${PKG_CONFIG_PATH:+:${PKG_CONFIG_PATH}}"
export LD_LIBRARY_PATH="${GDAL_PREFIX}/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export PROJ_DATA="/usr/share/proj"

# Bindgen configuration for gdal-sys
if command -v llvm-config >/dev/null 2>&1; then
  export LIBCLANG_PATH="$(llvm-config --libdir)"
else
  for d in /usr/lib/llvm-*/lib; do
    if [ -d "$d" ]; then
      export LIBCLANG_PATH="$d"
      break
    fi
  done
fi
export BINDGEN_EXTRA_CLANG_ARGS="-I${GDAL_PREFIX}/include"

# If running in GitHub Actions, also write to GITHUB_ENV
if [ -n "${GITHUB_ENV:-}" ]; then
  {
    echo "GDAL_PREFIX=${GDAL_PREFIX}"
    echo "GDAL_CONFIG=${GDAL_CONFIG}"
    echo "GDAL_DATA=${GDAL_DATA}"
    echo "GDAL_INCLUDE_DIR=${GDAL_INCLUDE_DIR}"
    echo "GDAL_LIB_DIR=${GDAL_LIB_DIR}"
    echo "PKG_CONFIG_PATH=${PKG_CONFIG_PATH}"
    echo "LD_LIBRARY_PATH=${LD_LIBRARY_PATH}"
    echo "PROJ_DATA=${PROJ_DATA}"
    echo "LIBCLANG_PATH=${LIBCLANG_PATH:-}"
    echo "BINDGEN_EXTRA_CLANG_ARGS=${BINDGEN_EXTRA_CLANG_ARGS}"
  } >> "$GITHUB_ENV"
fi
