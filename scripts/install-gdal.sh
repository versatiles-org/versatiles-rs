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
#   --testing   On Debian stable, add the testing repo as a pin-priority
#               source and install GDAL from there (for a newer version).
set -euo pipefail

# ── Required GDAL version (major.minor) ──
GDAL_REQUIRED="3.12"

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
ok()    { printf "\033[1;32m[+] %s\033[0m\n" "$*"; }
die()   { printf "\033[1;31m[x] %s\033[0m\n" "$*" >&2; exit 1; }

install_debian() {
  info "Installing GDAL ${GDAL_REQUIRED}.* via apt…"

  if [[ "$USE_TESTING" == "1" ]]; then
    info "Adding Debian testing repo with low pin priority…"
    sudo apt-get install -y debian-archive-keyring
    echo 'deb [signed-by=/usr/share/keyrings/debian-archive-keyring.gpg] http://deb.debian.org/debian testing main' \
      | sudo tee /etc/apt/sources.list.d/testing.list >/dev/null
    printf 'Package: *\nPin: release a=testing\nPin-Priority: 100\n' \
      | sudo tee /etc/apt/preferences.d/testing.pref >/dev/null
    sudo apt-get update
    APT_FLAGS="-t testing"
  else
    sudo apt-get update
    APT_FLAGS=""
  fi

  # shellcheck disable=SC2086
  if ! sudo apt-get install -y $APT_FLAGS "libgdal-dev=${GDAL_REQUIRED}.*" "gdal-bin=${GDAL_REQUIRED}.*" 2>/dev/null; then
    AVAILABLE="$(apt-cache policy libgdal-dev 2>/dev/null | head -5 || true)"
    die "GDAL ${GDAL_REQUIRED}.* is not available via apt.
Available versions:
${AVAILABLE}

Hints:
  - On Debian stable, try:  $0 --testing
  - On Ubuntu, you may need the 'ubuntugis' PPA."
  fi
}

install_alpine() {
  info "Installing GDAL ${GDAL_REQUIRED}.* via apk…"
  if ! apk add --no-cache "gdal-dev~${GDAL_REQUIRED}" 2>/dev/null; then
    AVAILABLE="$(apk policy gdal-dev 2>/dev/null || true)"
    die "GDAL ${GDAL_REQUIRED}.* is not available via apk.
Available versions:
${AVAILABLE}"
  fi
}

install_macos() {
  info "Installing GDAL ${GDAL_REQUIRED}.* via Homebrew…"
  if ! brew install "gdal@${GDAL_REQUIRED}" 2>/dev/null; then
    info "Versioned formula gdal@${GDAL_REQUIRED} not found, trying 'gdal'…"
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
      die "Unsupported Linux distribution. Install libgdal-dev ${GDAL_REQUIRED}.* manually."
    fi
    ;;
  Darwin)
    install_macos
    ;;
  *)
    die "Unsupported OS: $(uname -s)"
    ;;
esac

GDAL_VERSION="$(gdal-config --version 2>/dev/null || echo 'unknown')"
case "$GDAL_VERSION" in
  "${GDAL_REQUIRED}".*) ;;
  *) die "Expected GDAL ${GDAL_REQUIRED}.*, but got ${GDAL_VERSION}." ;;
esac
ok "GDAL installed successfully (${GDAL_VERSION})"
