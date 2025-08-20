#!/usr/bin/env bash
# Prepare environment variables for building Rust crates that rely on GDAL.
#
# The script detects the OS at runtime and exports the environment variables
# that gdal-sys’s build script understands (GDAL_HOME, GDAL_INCLUDE_DIR,
# GDAL_LIB_DIR, GDAL_VERSION).  On macOS an extra rpath is added so the
# dynamic loader can find libgdal.dylib at run‑time.

kernel_name="$(uname -s)"

case "$kernel_name" in
  Darwin)
    # Prefer the GDAL that gdal-config points to (Homebrew or custom build).
    if command -v gdal-config >/dev/null 2>&1; then
      GDAL_PREFIX="$(gdal-config --prefix)"
      export GDAL_HOME="$GDAL_PREFIX"
      export GDAL_INCLUDE_DIR="$GDAL_PREFIX/include"
      export GDAL_LIB_DIR="$GDAL_PREFIX/lib"
      # Make discovery explicit for gdal-sys and friends
      export GDAL_CONFIG="$(command -v gdal-config)"
      export PKG_CONFIG_PATH="$GDAL_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
      # Ensure runtime can find libgdal without extra setup
      export RUSTFLAGS='-C link-args=-Wl,-rpath,'"$GDAL_PREFIX"'/lib'
      export RUSTDOCFLAGS="$RUSTFLAGS"
    else
      echo "gdal-config not found. Please install GDAL (e.g., via Homebrew) before running this script." >&2
      exit 1
    fi

    # If a Conda environment is active, prevent it from hijacking the link step.
    for var in LDFLAGS LIBRARY_PATH CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH; do
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
    GDAL_PREFIX="$(gdal-config --prefix)"
    export GDAL_HOME="$GDAL_PREFIX"
    export GDAL_INCLUDE_DIR="$GDAL_PREFIX/include"
    export GDAL_LIB_DIR="$GDAL_PREFIX/lib"
    export GDAL_CONFIG="$(command -v gdal-config)"
    export PKG_CONFIG_PATH="$GDAL_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
    # No special RUSTFLAGS needed; the dynamic linker already searches /usr/lib.
    ;;

  *)
    echo "Unsupported operating system: $kernel_name" >&2
    exit 1
    ;;
esac

# GDAL version is useful for selecting the matching gdal-sys feature.
export GDAL_VERSION="$(gdal-config --version)"

echo "Configured GDAL ${GDAL_VERSION} (home: ${GDAL_HOME}; gdal-config: ${GDAL_CONFIG})"
unset kernel_name  # keep the user environment clean
