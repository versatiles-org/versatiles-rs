#!/usr/bin/env bash
# Prepare environment variables for building Rust crates that rely on GDAL.
#
# Supported setups
# ----------------
# 1. macOS + Conda
#    - `brew install --cask anaconda`
#    - `conda install -c conda-forge gdal libgdal`
#
# 2. Debian/Ubuntu + APT
#    - `sudo apt-get install gdal-bin libgdal-dev`
#
# The script detects the OS at runtime and exports the environment variables
# that gdal-sys’s build script understands (GDAL_HOME, GDAL_INCLUDE_DIR,
# GDAL_LIB_DIR, GDAL_VERSION).  On macOS an extra rpath is added so the
# dynamic loader can find libgdal.dylib at run‑time.

kernel_name="$(uname -s)"

case "$kernel_name" in
  Darwin)
    # ---------- macOS (Conda) -------------------------------------------------
    if [[ -z "${CONDA_PREFIX:-}" ]]; then
      echo "ERROR: Activate the Conda environment that contains GDAL first." >&2
      exit 1
    fi

    export GDAL_HOME="$CONDA_PREFIX"
    export GDAL_INCLUDE_DIR="$CONDA_PREFIX/include"
    export GDAL_LIB_DIR="$CONDA_PREFIX/lib"
    export RUSTFLAGS='-C link-args=-Wl,-rpath,'"$CONDA_PREFIX"'/lib'
    export RUSTDOCFLAGS="$RUSTFLAGS"
    ;;

  Linux)
    # ---------- Debian/Ubuntu (APT) ------------------------------------------
    GDAL_PREFIX="$(gdal-config --prefix)"
    export GDAL_HOME="$GDAL_PREFIX"
    export GDAL_INCLUDE_DIR="$GDAL_PREFIX/include"
    export GDAL_LIB_DIR="$GDAL_PREFIX/lib"
    # No special RUSTFLAGS needed; the dynamic linker already searches /usr/lib.
    ;;

  *)
    echo "Unsupported operating system: $kernel_name" >&2
    exit 1
    ;;
esac

# GDAL version is useful for selecting the matching gdal-sys feature.
export GDAL_VERSION="$(gdal-config --version)"

echo "Configured GDAL ${GDAL_VERSION} (home: ${GDAL_HOME})"
unset kernel_name  # keep the user environment clean
