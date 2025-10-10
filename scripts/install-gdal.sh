#!/usr/bin/env bash
# shellcheck shell=bash
# -----------------------------------------------------------------------------
# Build & install the latest stable GDAL release from source for THIS PROJECT.
# - Installs into: <repo>/.toolchain/gdal
# - Uses Homebrew (macOS) or APT (Debian/Ubuntu) for dependencies
# - Keeps runtime/env logic consistent and self-tested
#
# Usage:
#   bash scripts/install-gdal.sh [--jobs N] [--skip-deps] [--src-dir DIR] [--no-test]
# -----------------------------------------------------------------------------
set -euo pipefail

# ===== User-tunable config (via env or flags) =================================
: "${JOBS:=}"                 # Parallel build jobs (auto if empty)
: "${SKIP_DEPS:=0}"           # 1 = do not install deps
: "${SRC_DIR:=/tmp/gdal-src}" # Working dir for downloading/building
: "${INSTALL_TEST:=1}"        # 1 = run post-install sanity checks
# =============================================================================

# ----- tiny logger helpers ----------------------------------------------------
info() { printf "\033[1;36m==> %s\033[0m\n" "$*"; }
ok()   { printf "\033[1;32m[+] %s\033[0m\n" "$*"; }
warn() { printf "\033[1;33m[!] %s\033[0m\n" "$*"; }
die()  { printf "\033[1;31m[x] %s\033[0m\n" "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "Missing required tool: $1"; }

# ----- usage / arg parsing ----------------------------------------------------
usage() {
  cat <<EOF
Usage: $(basename "$0") [options]

Options:
  --jobs N         Parallel build jobs (default: auto)
  --skip-deps      Skip dependency installation
  --src-dir DIR    Working directory for sources (default: /tmp/gdal-src)
  --no-test        Skip post-install checks
  -h, --help       Show this help
EOF
}
while [[ $# -gt 0 ]]; do
  case "$1" in
    --jobs) JOBS="$2"; shift 2;;
    --skip-deps) SKIP_DEPS=1; shift;;
    --src-dir) SRC_DIR="$2"; shift 2;;
    --no-test) INSTALL_TEST=0; shift;;
    -h|--help) usage; exit 0;;
    *) die "Unknown argument: $1 (see --help)";;
  esac
done

# ----- platform detection -----------------------------------------------------
detect_platform() {
  case "$(uname -s)" in
    Darwin) PLATFORM=mac ;;
    Linux)  PLATFORM=linux ;;
    *)      die "Unsupported OS: $(uname -s)" ;;
  esac
}

# ----- project root & fixed install prefix --------------------------------------
resolve_project_root_and_prefix() {
  local script_dir root cand i
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

  # Typical layout is <root>/scripts/install-gdal.sh, so try parent first
  cand="$(cd "${script_dir}/.." && pwd)"

  # Walk up to 5 levels looking for markers
  root=""
  for ((i=0; i<5; i++)); do
    if [[ -f "${cand}/Cargo.lock" ]]; then
      root="${cand}"
      break
    fi
    # stop if we reached filesystem root
    [[ "${cand}" == "/" ]] && break
    cand="$(cd "${cand}/.." && pwd)"
  done

  if [[ -z "${root}" ]]; then
    # Fallback: assume parent of script_dir is the repo root
    root="$(cd "${script_dir}/.." && pwd)"
    warn "No project markers found (Cargo.lock). Falling back to: ${root}"
  fi

  REPO_ROOT="${root}"
  PREFIX="${REPO_ROOT}/.toolchain/gdal"
}

# ----- dependency installation (mac) -----------------------------------------
install_deps_mac() {
  info "Installing build dependencies via Homebrew…"
  need git brew curl
  HOMEBREW_PREFIX="$(brew --prefix)"
  brew update
  brew install \
    apache-arrow \
    ccache \
    cmake \
    expat \
    geos \
    giflib \
    jpeg-xl \
    json-c \
    libjpeg \
    libpng \
    libtiff \
    llvm \
    make \
    muparser \
    openjpeg \
    pkg-config \
    postgresql@16 \
    proj \
    sqlite \
    webp \
    xz \
    zstd \
    || true
  # Build-time discovery for CMake/pkg-config
  export CMAKE_PREFIX_PATH="${HOMEBREW_PREFIX}:${CMAKE_PREFIX_PATH:-}"
  export PKG_CONFIG_PATH="${HOMEBREW_PREFIX}/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
}

# ----- dependency installation (apt) -----------------------------------------
install_deps_apt() {
  info "Installing build dependencies via apt…"
  apt-get update
  apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    ccache \
    cmake \
    curl \
    git \
    libcurl4-openssl-dev \
    libexpat1-dev \
    libgeos-dev \
    libgeotiff-dev \
    libgif-dev \
    libjpeg-dev \
    libjson-c-dev \
    libjxl-dev \
    liblzma-dev \
    libmuparser-dev \
    libopenjp2-7-dev \
    libpng-dev \
    libpq-dev \
    libproj-dev \
    libsqlite3-dev \
    libtiff-dev \
    libwebp-dev \
    libzstd-dev \
    make \
    pkg-config \
    proj-bin \
    zlib1g-dev \
    || true
}

# ----- dependency installation (wrapper) -------------------------------------
install_deps() {
  detect_platform
  if [[ "$PLATFORM" == "mac" ]]; then
    install_deps_mac
  else
    install_deps_apt
  fi
}

# ----- build-time env (common) ------------------------------------------------
set_build_env() {
  # Derive sensible defaults once
  if [[ -z "${JOBS}" ]]; then
    if [[ "$PLATFORM" == "mac" ]]; then
      JOBS=$(sysctl -n hw.ncpu)
    else
      JOBS=$(nproc)
    fi
  fi
  ok "Parallel jobs: ${JOBS}"
}

# ----- runtime env (for tests) ------------------------------------------------
set_runtime_env() {
  # Make sure test binaries can find libs & data directories
  if [[ "$PLATFORM" == "mac" ]]; then
    local brew_proj_prefix
    brew_proj_prefix="$(brew --prefix proj 2>/dev/null || brew --prefix 2>/dev/null || echo /opt/homebrew)"
    export DYLD_LIBRARY_PATH="${PREFIX}/lib:${brew_proj_prefix%/proj}/lib:${DYLD_LIBRARY_PATH:-}"
    export PROJ_DATA="${brew_proj_prefix}/share/proj"
  else
    export LD_LIBRARY_PATH="${PREFIX}/lib:${LD_LIBRARY_PATH:-}"
    export PROJ_DATA="/usr/share/proj"
  fi
  unset PROJ_LIB
  export GDAL_DATA="${PREFIX}/share/gdal"
}

# ----- source dir prep --------------------------------------------------------
prepare_source_dir() {
  info "Preparing source directory: ${SRC_DIR}"
  rm -rf "$SRC_DIR" && mkdir -p "$SRC_DIR"
}

# ----- resolve latest GDAL version -------------------------------------------
resolve_gdal_version() {
  info "Resolving latest GDAL version via ls-remote…"
  GDAL_VERSION="$((git ls-remote --tags --refs https://github.com/OSGeo/gdal.git \
    | awk -F/ '/refs\/tags\/v[0-9]+\.[0-9]+\.[0-9]+$/ {print $NF}' \
    | sort -V | tail -n1) | sed 's/^v//')"
  [[ -n "$GDAL_VERSION" ]] || die "Could not determine the latest GDAL version"
  ok "Building GDAL version: ${GDAL_VERSION}"
}

# ----- fetch source tarball ---------------------------------------------------
fetch_source_tarball() {
  local tar="gdal-${GDAL_VERSION}.tar.gz"
  local url="https://github.com/OSGeo/gdal/archive/refs/tags/v${GDAL_VERSION}.tar.gz"
  cd "$SRC_DIR"
  if [[ ! -f "$tar" ]]; then
    info "Downloading $tar …"
    curl -fsSL -o "$tar.tmp" "$url" || die "curl failed to fetch $url"
    mv "$tar.tmp" "$tar"
  fi
  if [[ ! -d "gdal-${GDAL_VERSION}" ]]; then
    info "Extracting $tar …"
    tar xzf "$tar"
  fi
  cd "gdal-${GDAL_VERSION}"
}

# ----- configure build (assemble CMake args) ---------------------------------
configure_build() {
  info "Configuring CMake …"
  BUILD_DIR="$PWD/build"
  rm -rf "$BUILD_DIR" && mkdir -p "$BUILD_DIR"

  CMAKE_ARGS=(
    -DCMAKE_BUILD_TYPE=Release
    -DCMAKE_C_COMPILER_LAUNCHER=ccache
    -DCMAKE_CXX_COMPILER_LAUNCHER=ccache
    -DCMAKE_INSTALL_PREFIX="${PREFIX}"
    -DBUILD_TESTING=OFF
    # Disable Python SWIG bindings
    -DGDAL_ENABLE_SWIG=OFF
    -DGDAL_ENABLE_PYTHON=OFF
    -DGDAL_ENABLE_JAVA=OFF
    -DBUILD_PYTHON_BINDINGS=OFF
    -DBUILD_JAVA_BINDINGS=OFF
    # Keep TIFF internal (with WebP/JXL enabled via external deps)
    -DGDAL_USE_TIFF_INTERNAL=ON
    -DGDAL_USE_TIFF=ON
    -DGDAL_USE_WEBP=ON
    -DGDAL_USE_JXL=ON
    # Common toggles we don’t need (trim build time/size)
    -DGDAL_USE_ARROW=OFF
    -DGDAL_USE_PARQUET=OFF
    -DGDAL_USE_POPPLER=OFF
    -DGDAL_USE_SFCGAL=OFF
    -DGDAL_USE_GEOTIFF_INTERNAL=ON
    -DGDAL_USE_GEOTIFF=ON
    -DGDAL_USE_JSONC=ON
    -DGDAL_USE_INTERNAL_LIBS=WHEN_NO_EXTERNAL
  )

  if [[ "$PLATFORM" == "mac" ]]; then
    local brew_prefix
    brew_prefix="$(brew --prefix)"
    CMAKE_ARGS+=(
      -DCMAKE_PREFIX_PATH="${brew_prefix}"
      -DCMAKE_MACOSX_RPATH=ON
      -DCMAKE_INSTALL_RPATH="${PREFIX}/lib"
      -DCMAKE_BUILD_RPATH="${PREFIX}/lib"
      -DCMAKE_INSTALL_RPATH_USE_LINK_PATH=ON
      -DCMAKE_DISABLE_FIND_PACKAGE_Arrow=ON
    )
  fi

  cmake -S . -B "$BUILD_DIR" "${CMAKE_ARGS[@]}"
}

# ----- build & install --------------------------------------------------------
build_and_install() {
  info "Building …"
  cmake --build "$BUILD_DIR" -- -j"${JOBS}"

  info "Installing (no sudo if prefix writable) …"
  mkdir -p "$PREFIX" || true
  if [[ -w "$PREFIX" ]]; then
    cmake --install "$BUILD_DIR"
  else
    sudo cmake --install "$BUILD_DIR"
  fi

  # Linux only: refresh linker cache if relevant
  if [[ "$PLATFORM" == "linux" && -d /etc/ld.so.conf.d ]]; then
    sudo ldconfig || true
  fi
}

# ----- post-install checks ----------------------------------------------------
post_install_checks() {
  [[ "$INSTALL_TEST" == "1" ]] || { warn "Skipping post-install tests (--no-test)"; return; }
  info "Running post-install sanity checks …"

  # Prefer the just-installed tools
  local gdalinfo_bin="${PREFIX}/bin/gdalinfo"
  local gdalsrsinfo_bin="${PREFIX}/bin/gdalsrsinfo"
  local projinfo_bin
  projinfo_bin="$(command -v projinfo || true)"
  [[ -x "$gdalinfo_bin" ]] || gdalinfo_bin="$(command -v gdalinfo || true)"
  [[ -x "$gdalsrsinfo_bin" ]] || gdalsrsinfo_bin="$(command -v gdalsrsinfo || true)"

  if [[ -z "$gdalinfo_bin" ]]; then
    warn "gdalinfo not found. Add ${PREFIX}/bin to PATH."
    return
  fi

  set_runtime_env
  ok   "Using gdalinfo: $gdalinfo_bin"
  [[ -n "$gdalsrsinfo_bin" ]] && ok "Using gdalsrsinfo: $gdalsrsinfo_bin" || warn "gdalsrsinfo not found"
  [[ -n "$projinfo_bin"    ]] && ok "Using projinfo: $projinfo_bin"       || warn "projinfo not found"

  "$gdalinfo_bin" --version || true

  # Data directory diagnostics
  if [[ -d "${PROJ_DATA}" ]]; then
    ok "PROJ_DATA=${PROJ_DATA}"
    [[ -f "${PROJ_DATA}/proj.db" ]] && ok "proj.db OK" || warn "proj.db missing under PROJ_DATA"
  else
    warn "PROJ_DATA directory missing: ${PROJ_DATA}"
  fi

  # EPSG lookups (non-fatal)
  if [[ -n "$projinfo_bin" ]]; then
    "$projinfo_bin" EPSG:4326 >/dev/null 2>&1 || warn "projinfo EPSG:4326 failed"
  fi
  if [[ -n "$gdalsrsinfo_bin" ]]; then
    "$gdalsrsinfo_bin" EPSG:4326 >/dev/null 2>&1 || warn "gdalsrsinfo EPSG:4326 failed"
  fi
}

# ===== main flow ==============================================================
main() {
  detect_platform
  resolve_project_root_and_prefix
  [[ "$PLATFORM" == "mac" ]] || [[ "$PLATFORM" == "linux" ]] || die "Unsupported platform"

  if [[ "$SKIP_DEPS" != "1" ]]; then
    install_deps
  else
    warn "Skipping dependency installation (--skip-deps)"
  fi

  set_build_env
  prepare_source_dir
  resolve_gdal_version
  fetch_source_tarball
  configure_build
  build_and_install
  post_install_checks
  ok "GDAL ${GDAL_VERSION} installed to ${PREFIX}"
}

# Run only if executed directly, not sourced
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
