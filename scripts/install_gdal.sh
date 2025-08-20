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

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "Missing required tool: $1"; }

OS="$(uname -s)"
case "$OS" in
  Darwin) PLATFORM=mac ;;
  Linux)  PLATFORM=linux ;;
  *)      die "Unsupported OS: $OS" ;;
esac

# Detect package manager & set defaults
if [[ "$PLATFORM" == "mac" ]]; then
  if ! command -v brew >/dev/null 2>&1; then
    die "Homebrew is required but not found. Install from https://brew.sh/ then re-run."
  fi
  need_cmd git
  need_cmd cmake
  need_cmd make
  HOMEBREW_PREFIX="$(brew --prefix)"
  : "${PREFIX:=${HOMEBREW_PREFIX}}"
  # Default jobs: number of hardware threads
  : "${JOBS:=$(sysctl -n hw.ncpu)}"
elif [[ "$PLATFORM" == "linux" ]]; then
  need_cmd git
  need_cmd cmake || true # we’ll install it if missing
  need_cmd make  || true
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
    cmake pkg-config \
    proj geos sqlite libtiff libjpeg libpng \
    webp openjpeg \
    zstd xz expat \
    curl json-c \
    postgresql@16 || true
  # Ensure pkg-config and CMake can find Homebrew libs
  export PKG_CONFIG_PATH="${HOMEBREW_PREFIX}/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
  export CMAKE_PREFIX_PATH="${HOMEBREW_PREFIX}:${CMAKE_PREFIX_PATH:-}"
}

install_deps_apt() {
  say "Installing build dependencies via apt… (may prompt for sudo)"
  sudo apt-get update
  sudo apt-get install -y --no-install-recommends \
    build-essential cmake pkg-config git \
    libproj-dev libgeos-dev libsqlite3-dev \
    libtiff-dev libjpeg-dev libpng-dev \
    libwebp-dev libopenjp2-7-dev \
    libzstd-dev liblzma-dev zlib1g-dev \
    libexpat1-dev \
    libjson-c-dev \
    libcurl4-openssl-dev \
    libpq-dev \
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
  -DCMAKE_INSTALL_PREFIX="${PREFIX}"
  -DGDAL_USE_INTERNAL_LIBS=OFF
  -DGDAL_USE_JSONC=ON
)

# Help CMake find brew-installed libraries on macOS
if [[ "$PLATFORM" == "mac" ]]; then
  CMAKE_ARGS+=(
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
    fi
    gdalinfo --version || true
  else
    warn "gdalinfo not found in PATH. You may need to add ${PREFIX}/bin to your PATH."
    warn "Example: export PATH=${PREFIX}/bin:\$PATH"
  fi
fi

say "GDAL ${GDAL_TAG} installed to ${PREFIX}. Done."