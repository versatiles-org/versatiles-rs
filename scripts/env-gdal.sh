#!/usr/bin/env bash
# Prepare environment variables for building Rust crates that rely on GDAL.
#
# The script detects the OS at runtime and exports the environment variables
# that gdal-sys’s build script understands (GDAL_HOME, GDAL_INCLUDE_DIR,
# GDAL_LIB_DIR, GDAL_VERSION).  On macOS an extra rpath is added so the
# dynamic loader can find libgdal.dylib at run‑time.

# Find project root by searching upward for Cargo.lock
find_project_root() {
	local dir="$PWD"
	while [ "$dir" != "/" ]; do
		if [ -f "$dir/Cargo.lock" ]; then
			echo "$dir"
			return 0
		fi
		dir="$(dirname "$dir")"
	done
	echo "❌ ERROR: Could not find project root (Cargo.lock not found)" >&2
	exit 1
}

PROJECT_DIR=$(find_project_root)

if [ ! -f "$PROJECT_DIR/.toolchain/gdal/bin/gdal-config" ]; then
  echo "❌ ERROR: GDAL is required but not installed"
  echo "   Please run: ./scripts/install-gdal.sh"
  exit 1
fi

kernel_name="$(uname -s)"

path_prepend_unique() {
  # POSIX/zsh-compatible (no indirect ${!var})
  var="$1"; dir="$2"
  eval "cur=\${$var:-}"
  case ":$cur:" in
    *":$dir:"*) return;; # already present
    *)
      if [ -n "$cur" ]; then
        eval "export $var=\"$dir:$cur\""
      else
        eval "export $var=\"$dir\""
      fi
      ;;
  esac
}

export GDAL_PREFIX="${PROJECT_DIR}/.toolchain/gdal"
export GDAL_HOME="${GDAL_PREFIX}"
export GDAL_CONFIG="${GDAL_PREFIX}/bin/gdal-config"
export GDAL_INCLUDE_DIR="${GDAL_PREFIX}/include"
export GDAL_DATA="${GDAL_HOME}/share/gdal"
export GDAL_LIB_DIR="${GDAL_PREFIX}/lib"

path_prepend_unique PKG_CONFIG_PATH "${GDAL_PREFIX}/lib/pkgconfig"
export PKG_CONFIG_PATH

case "$kernel_name" in
  Darwin)
    if command -v gdal-config >/dev/null 2>&1; then
      # Ensure runtime can find libgdal without extra setup
      export RUSTFLAGS='-C link-args=-Wl,-rpath,'"${GDAL_PREFIX}"'/lib'
      export RUSTDOCFLAGS="$RUSTFLAGS"

      # Set data directories for CRS lookup and ensure runtime loader finds GDAL
      export PROJ_PREFIX="$(pkg-config --variable=prefix proj 2>/dev/null || /opt/homebrew/bin/brew --prefix 2>/dev/null || echo /opt/homebrew)"
      
      unset PROJ_LIB
      path_prepend_unique DYLD_LIBRARY_PATH "${GDAL_LIB_DIR}"
      export DYLD_LIBRARY_PATH
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
    export PROJ_PREFIX="$(pkg-config --variable=prefix proj 2>/dev/null || echo /usr)"
    path_prepend_unique LD_LIBRARY_PATH "${GDAL_LIB_DIR}"
    export LD_LIBRARY_PATH
    unset PROJ_LIB
    ;;

  *)
    echo "Unsupported operating system: $kernel_name" >&2
    exit 1
    ;;
esac

export PROJ_DATA="${PROJ_PREFIX}/share/proj"

# GDAL version is useful for selecting the matching gdal-sys feature.
export GDAL_VERSION="$($GDAL_CONFIG --version 2>/dev/null || echo unknown)"

if [ -z "$GDAL_VERSION" ] || [ "$GDAL_VERSION" = "unknown" ]; then
  echo "Failed to determine GDAL version via gdal-config." >&2
  exit 1
fi

echo "Configured:"
echo "  GDAL_VERSION:      ${GDAL_VERSION:-unset}"
# echo "  GDAL_HOME:         ${GDAL_HOME:-unset}"
# echo "  GDAL_CONFIG:       ${GDAL_CONFIG:-unset}"
# echo "  PROJ_DATA:         ${PROJ_DATA:-unset}"
# echo "  GDAL_DATA:         ${GDAL_DATA:-unset}"

# case "$kernel_name" in
#   Darwin) echo "  DYLD_LIBRARY_PATH: ${DYLD_LIBRARY_PATH:-unset}" ;;
#   Linux)  echo "  LD_LIBRARY_PATH:   ${LD_LIBRARY_PATH:-unset}" ;;
#   *) : ;;
# esac

unset kernel_name  # keep the user environment clean
