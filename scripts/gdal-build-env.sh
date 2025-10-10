#!/usr/bin/env bash
# Prepare environment variables for building Rust crates that rely on GDAL.
#
# The script detects the OS at runtime and exports the environment variables
# that gdal-sys’s build script understands (GDAL_HOME, GDAL_INCLUDE_DIR,
# GDAL_LIB_DIR, GDAL_VERSION).  On macOS an extra rpath is added so the
# dynamic loader can find libgdal.dylib at run‑time.

cd "$(dirname "$0")/.."
PROJECT_DIR=$(pwd)

kernel_name="$(uname -s)"

export GDAL_PREFIX="$PROJECT_DIR/.toolchain/gdal"
export GDAL_HOME="$GDAL_PREFIX"
export GDAL_CONFIG=${GDAL_PREFIX}/bin/gdal-config
export GDAL_INCLUDE_DIR="$GDAL_PREFIX/include"
export GDAL_DATA="${GDAL_HOME}/share/gdal"
export GDAL_LIB_DIR="$GDAL_PREFIX/lib"

LD_LIBRARY_PATH=${GDAL_PREFIX}/lib:${LD_LIBRARY_PATH}
PROJ_DATA=/usr/share/proj
export PKG_CONFIG_PATH="$GDAL_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

case "$kernel_name" in
  Darwin)
    # Prefer the GDAL that gdal-config points to (Homebrew or custom build).
    if command -v gdal-config >/dev/null 2>&1; then
     
      # Ensure runtime can find libgdal without extra setup
      export RUSTFLAGS='-C link-args=-Wl,-rpath,'"$GDAL_PREFIX"'/lib'
      export RUSTDOCFLAGS="$RUSTFLAGS"

      # Set data directories for CRS lookup and ensure runtime loader finds GDAL
      PROJ_PREFIX="$(pkg-config --variable=prefix proj 2>/dev/null || /opt/homebrew/bin/brew --prefix 2>/dev/null || echo /opt/homebrew)"
      
      unset PROJ_LIB
      export DYLD_LIBRARY_PATH="${GDAL_LIB_DIR}:${DYLD_LIBRARY_PATH:-}"
    else
      echo "gdal-config not found. Please install GDAL (e.g., via Homebrew) before running this script." >&2
      exit 1
    fi

    # If a Conda environment is active, prevent it from hijacking the link step.
    for var in LDFLAGS LIBRARY_PATH CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH PROJ_DATA PROJ_LIB; do
      eval val=\"\${$var-}\"
      case "$val" in
        *"${CONDA_PREFIX:-/does/not/exist}"*|*/opt/anaconda3/*)
          unset "$var"
          ;;
      esac
    done
    ;;

  Linux)
    # ---------- Debian/Ubuntu (APT) ------------------------------------------
    PROJ_PREFIX="$(pkg-config --variable=prefix proj 2>/dev/null || echo /usr)"
    unset PROJ_LIB
    export LD_LIBRARY_PATH="${GDAL_LIB_DIR}:${LD_LIBRARY_PATH:-}"
    ;;

  *)
    echo "Unsupported operating system: $kernel_name" >&2
    exit 1
    ;;
esac

export PROJ_DATA="${PROJ_PREFIX}/share/proj"

# GDAL version is useful for selecting the matching gdal-sys feature.
export GDAL_VERSION="$(gdal-config --version)"

echo "Configured GDAL ${GDAL_VERSION} (home: ${GDAL_HOME}; gdal-config: ${GDAL_CONFIG})"
echo "Data dirs: PROJ_DATA=${PROJ_DATA:-unset}; GDAL_DATA=${GDAL_DATA:-unset}"
case "$kernel_name" in
  Darwin) echo "Runtime: DYLD_LIBRARY_PATH=${DYLD_LIBRARY_PATH:-unset}" ;;
  Linux)  echo "Runtime: LD_LIBRARY_PATH=${LD_LIBRARY_PATH:-unset}" ;;
  *) : ;;
esac
unset kernel_name  # keep the user environment clean