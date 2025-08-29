#!/usr/bin/env bash
# Build & install the latest GDAL release (from tags) on macOS (Homebrew) or Debian/Ubuntu.
# Usage:
#   PREFIX=/custom/prefix JOBS=8 bash install-gdal.sh
#   SKIP_DEPS=1 bash install-gdal.sh      # if you’ve already installed deps
#   GDAL_TAG=v3.9.1 bash install-gdal.sh  # build a specific tag instead of auto-latest
set -euo pipefail

### --- Configurable env vars ---
: "${PREFIX:=}"            # Default decided per-OS (Homebrew prefix on macOS, /usr/local on Linux)
: "${JOBS:=}"              # Default: auto-detect
: "${SKIP_DEPS:=0}"        # 1 to skip dependency installation
: "${GDAL_TAG:=}"          # e.g., v3.9.1 ; if empty we auto-pick the newest tag by creation date
: "${SRC_DIR:=/tmp/gdal-src}"  # where to clone/build
: "${INSTALL_TEST:=1}"     # run `gdalinfo --version` after installation
### -----------------------------

say()  { printf "\033[1;34m[+] %s\033[0m\n" "$*"; }
warn() { printf "\033[1;33m[!] %s\033[0m\n" "$*"; }
die()  { printf "\033[1;31m[x] %s\033[0m\n" "$*" >&2; exit 1; }

OS="$(uname -s)"
case "$OS" in
  Darwin) PLATFORM=mac ;;
  Linux)  PLATFORM=linux ;;
  *)      die "Unsupported OS: $OS" ;;
esac

command -v "git" >/dev/null 2>&1 || die "Missing required tool: git";

# Detect package manager & set defaults
if [[ "$PLATFORM" == "mac" ]]; then
  if ! command -v brew >/dev/null 2>&1; then
    die "Homebrew is required but not found. Install from https://brew.sh/ then re-run."
  fi
  HOMEBREW_PREFIX="$(brew --prefix)"
  : "${PREFIX:=${HOMEBREW_PREFIX}}"
  # Default jobs: number of hardware threads
  : "${JOBS:=$(sysctl -n hw.ncpu)}"
elif [[ "$PLATFORM" == "linux" ]]; then
  # Pick apt for Debian/Ubuntu
  if command -v apt-get >/dev/null 2>&1; then
    PKG=apt
  else
    die "Only Debian/Ubuntu (APT) is supported on Linux in this script."
  fi
  : "${PREFIX:=/usr/local}"
  : "${JOBS:=$(nproc)}"
fi

say "Platform: $PLATFORM"
say "Install prefix: $PREFIX"
say "Parallel jobs: ${JOBS}"

install_deps_mac() {
  say "Installing build dependencies via Homebrew…"
  # Core build + common optional libraries
  brew update
  brew install \
    cmake make pkg-config llvm ccache \
    proj geos sqlite libtiff libjpeg libpng \
    webp openjpeg \
    giflib \
    zstd xz expat \
    curl json-c \
    apache-arrow \
    postgresql@16 || true
  # Ensure pkg-config and CMake can find Homebrew libs
  export PKG_CONFIG_PATH="${HOMEBREW_PREFIX}/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export CMAKE_PREFIX_PATH="${HOMEBREW_PREFIX}:${CMAKE_PREFIX_PATH:-}"
}

install_deps_apt() {
  say "Installing build dependencies via apt… (may prompt for sudo)"
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends \
    build-essential cmake make pkg-config git ccache \
    libproj-dev libgeos-dev libsqlite3-dev \
    libgeotiff-dev libtiff-dev libjpeg-dev libpng-dev \
    libwebp-dev libopenjp2-7-dev \
    libgif-dev \
    libzstd-dev liblzma-dev zlib1g-dev \
    libexpat1-dev \
    libjson-c-dev \
    libcurl4-openssl-dev \
    libpq-dev \
    proj-bin \
    ca-certificates
}

if [[ "$SKIP_DEPS" != "1" ]]; then
  if [[ "$PLATFORM" == "mac" ]]; then
    install_deps_mac
  else
    install_deps_apt
  fi
else
  warn "Skipping dependency installation as requested (SKIP_DEPS=1)."
fi

# Prepare source directory
say "Setting up source directory at ${SRC_DIR}…"
mkdir -p "$SRC_DIR"
cd "$SRC_DIR"

# Clone or update GDAL
if [[ -d gdal/.git ]]; then
  say "Updating existing GDAL repo…"
  cd gdal
  git fetch --tags --prune
else
  say "Cloning GDAL repository…"
  git clone https://github.com/OSGeo/gdal.git
  cd gdal
fi

# Resolve tag to build
if [[ -z "$GDAL_TAG" ]]; then
  say "Selecting the newest GDAL tag by creation date…"
  # Pick the newest tag (works on both GNU/BSD sort environments)
  LATEST_TAG="$(git for-each-ref --sort=-creatordate --format='%(refname:short)' refs/tags | head -n1)"
  if [[ -z "$LATEST_TAG" ]]; then
    die "Could not determine latest tag. Repo might not have tags?"
  fi
  GDAL_TAG="$LATEST_TAG"
fi
say "Building GDAL tag: $GDAL_TAG"
git checkout -q "$GDAL_TAG"

# Build
say "Configuring with CMake…"
BUILD_DIR="$PWD/build"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# On macOS, prefer Homebrew prefix for install and dependency discovery.
CMAKE_ARGS=(
  -DCMAKE_BUILD_TYPE=Release
  -DCMAKE_C_COMPILER_LAUNCHER=ccache
  -DCMAKE_CXX_COMPILER_LAUNCHER=ccache
  -DCMAKE_INSTALL_PREFIX="${PREFIX}"
  -DGDAL_USE_ARROW=OFF
  -DGDAL_USE_GEOTIFF_INTERNAL=ON
  -DGDAL_USE_GEOTIFF=ON
  -DGDAL_USE_INTERNAL_LIBS=WHEN_NO_EXTERNAL
  -DGDAL_USE_JSONC=ON
  -DGDAL_USE_PARQUET=OFF
  -DGDAL_USE_SFCGAL=OFF
  -DGDAL_USE_TIFF_INTERNAL=ON
  -DGDAL_USE_TIFF=ON
  -DGDAL_USE_WEBP=ON
)

# Help CMake find brew-installed libraries on macOS
if [[ "$PLATFORM" == "mac" ]]; then
  CMAKE_ARGS+=(
    "-DCMAKE_DISABLE_FIND_PACKAGE_Arrow=ON"
    "-DCMAKE_PREFIX_PATH=${HOMEBREW_PREFIX}"
    "-DCMAKE_MACOSX_RPATH=ON"
    "-DCMAKE_INSTALL_RPATH=${PREFIX}/lib"
    "-DCMAKE_BUILD_RPATH=${PREFIX}/lib"
    "-DCMAKE_INSTALL_RPATH_USE_LINK_PATH=ON"
  )
fi

cmake .. "${CMAKE_ARGS[@]}"

say "Compiling…"
cmake --build . -- -j"${JOBS}"

say "Installing (may prompt for sudo if PREFIX not writable)…"
if [[ -w "$PREFIX" ]]; then
  cmake --install .
else
  sudo cmake --install .
fi

# Linux: refresh linker cache if we installed into a standard lib dir
if [[ "$PLATFORM" == "linux" ]]; then
  if [[ -d "/etc/ld.so.conf.d" ]]; then
    sudo ldconfig || true
  fi
fi

# Post-install check
if [[ "$INSTALL_TEST" == "1" ]]; then
  say "Testing installation: gdalinfo --version"
  if command -v gdalinfo >/dev/null 2>&1; then
    if [[ "$PLATFORM" == "mac" ]]; then
      export DYLD_LIBRARY_PATH="${PREFIX}/lib:${DYLD_LIBRARY_PATH:-}"
      # Ensure data dirs are set for CRS lookups
      export PROJ_DATA="${HOMEBREW_PREFIX}/share/proj"
    else
      # Ensure data dirs are set for CRS lookups
      export PROJ_DATA="/usr/share/proj"
    fi
    unset PROJ_LIB
    export GDAL_DATA="${PREFIX}/share/gdal"
    gdalinfo --version || true
    command -v projinfo >/dev/null 2>&1 && projinfo EPSG:4326 || true
    command -v gdalsrsinfo >/dev/null 2>&1 && gdalsrsinfo EPSG:4326 >/dev/null 2>&1 || warn "EPSG lookup failed; check PROJ_DATA (${PROJ_DATA}) and GDAL_DATA (${GDAL_DATA})."
  else
    warn "gdalinfo not found in PATH. You may need to add ${PREFIX}/bin to your PATH."
    warn "Example: export PATH=${PREFIX}/bin:\$PATH"
  fi
fi

say "GDAL ${GDAL_TAG} installed to ${PREFIX}. Done."

#!/usr/bin/env bash
# Set up environment variables for GDAL build and runtime.

OS="$(uname -s)"

case "$OS" in
  Darwin)
    # macOS: set PROJ and GDAL data dirs, unset PROJ_LIB
    PROJ_PREFIX="$(brew --prefix proj)"
    GDAL_HOME="$(brew --prefix gdal 2>/dev/null || echo "${PROJ_PREFIX}")"
    export PROJ_DATA="${PROJ_PREFIX}/share/proj"
    export GDAL_DATA="${GDAL_HOME}/share/gdal"
    unset PROJ_LIB

    # Sanity check: ensure proj.db exists where we point PROJ_DATA
    if [[ ! -f "${PROJ_DATA}/proj.db" ]]; then
      echo "[warn] PROJ_DATA=${PROJ_DATA} does not contain proj.db; 'projinfo EPSG:4326' will likely fail." >&2
    fi

    # Unset Conda-related env vars that might interfere, include PROJ_DATA and DYLD_FALLBACK_LIBRARY_PATH
    for var in LDFLAGS LIBRARY_PATH CPATH C_INCLUDE_PATH CPLUS_INCLUDE_PATH PROJ_DATA PROJ_LIB DYLD_FALLBACK_LIBRARY_PATH; do
      unset "$var"
    done
    ;;

  Linux)
    # Linux: set PROJ and GDAL data dirs, unset PROJ_LIB
    GDAL_HOME="${GDAL_HOME:-$(gdal-config --prefix 2>/dev/null || echo /usr/local)}"
    PROJ_PREFIX="$(pkg-config --variable=prefix proj 2>/dev/null || echo /usr)"
    export PROJ_DATA="${PROJ_PREFIX}/share/proj"
    export GDAL_DATA="${GDAL_HOME}/share/gdal"
    unset PROJ_LIB

    # Other Linux environment setup can go here
    ;;

  *)
    echo "Unsupported OS: $OS"
    exit 1
    ;;
esac

echo "Configured GDAL environment for $OS."
echo "Data dirs: PROJ_DATA=${PROJ_DATA:-unset}; GDAL_DATA=${GDAL_DATA:-unset}"

# Quick live checks (non-fatal)
command -v projinfo >/dev/null 2>&1 && projinfo EPSG:4326 >/dev/null 2>&1 || echo "[warn] projinfo EPSG:4326 failed; check PROJ_DATA=${PROJ_DATA}"
command -v gdalsrsinfo >/dev/null 2>&1 && gdalsrsinfo EPSG:4326 >/dev/null 2>&1 || echo "[warn] gdalsrsinfo EPSG:4326 failed; check GDAL_DATA=${GDAL_DATA}"