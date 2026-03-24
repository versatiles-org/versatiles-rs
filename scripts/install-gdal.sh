#!/usr/bin/env bash
# Install GDAL development libraries via the system package manager.
#
# Supported platforms:
#   - Debian/Ubuntu (apt)
#   - Alpine (apk)
#   - macOS (Homebrew)
#
# Usage:
#   ./scripts/install-gdal.sh [--testing]
#
# Options:
#   --testing   On Debian stable, add the testing repo and install GDAL
#               (+ its dependencies) from there. This may upgrade core
#               system libraries (libc, libstdc++, etc.) to testing
#               versions. Safe for CI containers; use with care on
#               persistent dev machines.
#
# The script tries to install GDAL GDAL_PREFERRED first. If that version is
# not available, it falls back to whatever the package manager provides and
# prints a warning instead of failing.
set -euo pipefail

# ── GDAL versions ──
GDAL_PREFERRED="3.12"   # try this version first
GDAL_MIN_MAJOR=3         # minimum acceptable: 3.4
GDAL_MIN_MINOR=4

USE_TESTING=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --testing) USE_TESTING=1; shift ;;
    -h|--help)
      echo "Usage: $(basename "$0") [--testing]"
      echo "  --testing  Install GDAL from Debian testing repo (Debian/Ubuntu only)"
      exit 0 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

info()  { printf "\033[1;36m==> %s\033[0m\n" "$*"; }
warn()  { printf "\033[1;33m[!] %s\033[0m\n" "$*"; }
ok()    { printf "\033[1;32m[+] %s\033[0m\n" "$*"; }
die()   { printf "\033[1;31m[x] %s\033[0m\n" "$*" >&2; exit 1; }

install_debian() {
  if [[ "$USE_TESTING" == "1" ]]; then
    info "Adding Debian testing repo…"
    sudo apt-get install -y debian-archive-keyring
    echo 'deb [signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] http://deb.debian.org/debian testing main' \
      | sudo tee /etc/apt/sources.list.d/testing.list >/dev/null
    # Pin testing below default so nothing upgrades automatically
    printf 'Package: *\nPin: release a=testing\nPin-Priority: 10\n' \
      | sudo tee /etc/apt/preferences.d/testing.pref >/dev/null
  fi

  sudo apt-get update

  # Try preferred version first
  if [[ "$USE_TESTING" == "1" ]]; then
    # Use -t testing so apt resolves the full dependency tree from testing
    info "Installing GDAL ${GDAL_PREFERRED}.* from testing (including dependencies)…"
    if sudo apt-get install -y -t testing "libgdal-dev=${GDAL_PREFERRED}.*" "gdal-bin=${GDAL_PREFERRED}.*" libclang-dev 2>/dev/null; then
      return 0
    fi
  else
    info "Trying to install GDAL ${GDAL_PREFERRED}.* via apt…"
    if sudo apt-get install -y "libgdal-dev=${GDAL_PREFERRED}.*" "gdal-bin=${GDAL_PREFERRED}.*" libclang-dev 2>/dev/null; then
      return 0
    fi
  fi

  # Fall back to whatever is available
  warn "GDAL ${GDAL_PREFERRED}.* not available via apt, installing default version…"
  sudo apt-get install -y libgdal-dev gdal-bin libclang-dev
}

install_alpine() {
  # Try preferred version first
  info "Trying to install GDAL ${GDAL_PREFERRED}.* via apk…"
  if apk add --no-cache "gdal-dev~${GDAL_PREFERRED}" 2>/dev/null; then
    return 0
  fi

  # Fall back to whatever is available
  warn "GDAL ${GDAL_PREFERRED}.* not available via apk, installing default version…"
  apk add --no-cache gdal-dev
}

install_macos() {
  # Homebrew typically only provides an unversioned "gdal" formula.
  # Check if a versioned formula exists before trying it.
  if brew info "gdal@${GDAL_PREFERRED}" &>/dev/null; then
    info "Installing GDAL ${GDAL_PREFERRED} via Homebrew…"
    brew install "gdal@${GDAL_PREFERRED}"
  else
    info "Installing GDAL via Homebrew…"
    brew install gdal
  fi
}

# Detect platform and install
case "$(uname -s)" in
  Linux)
    if [ -f /etc/alpine-release ]; then
      install_alpine
    elif command -v apt-get >/dev/null 2>&1; then
      install_debian
    else
      die "Unsupported Linux distribution. Install libgdal-dev manually."
    fi
    ;;
  Darwin)
    install_macos
    ;;
  *)
    die "Unsupported OS: $(uname -s)"
    ;;
esac

# Verify minimum version
GDAL_VERSION="$(gdal-config --version 2>/dev/null || echo 'unknown')"
if [[ "$GDAL_VERSION" == "unknown" ]]; then
  die "gdal-config not found after installation."
fi

MAJOR="$(echo "$GDAL_VERSION" | cut -d. -f1)"
MINOR="$(echo "$GDAL_VERSION" | cut -d. -f2)"
if [[ "$MAJOR" -lt "$GDAL_MIN_MAJOR" ]] || { [[ "$MAJOR" -eq "$GDAL_MIN_MAJOR" ]] && [[ "$MINOR" -lt "$GDAL_MIN_MINOR" ]]; }; then
  die "GDAL >= ${GDAL_MIN_MAJOR}.${GDAL_MIN_MINOR} required, but got ${GDAL_VERSION}."
fi

case "$GDAL_VERSION" in
  "${GDAL_PREFERRED}".*) ok "GDAL installed successfully (${GDAL_VERSION})" ;;
  *) warn "Wanted GDAL ${GDAL_PREFERRED}.*, but got ${GDAL_VERSION}. Continuing anyway."
     ok "GDAL installed (${GDAL_VERSION})" ;;
esac
