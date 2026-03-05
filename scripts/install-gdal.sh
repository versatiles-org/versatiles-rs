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
  info "Installing GDAL via apt…"

  if [[ "$USE_TESTING" == "1" ]]; then
    info "Adding Debian testing repo with low pin priority…"
    echo 'deb http://deb.debian.org/debian testing main' \
      | sudo tee /etc/apt/sources.list.d/testing.list >/dev/null
    printf 'Package: *\nPin: release a=testing\nPin-Priority: 100\n' \
      | sudo tee /etc/apt/preferences.d/testing.pref >/dev/null
    sudo apt-get update
    sudo apt-get install -y -t testing libgdal-dev gdal-bin
  else
    sudo apt-get update
    sudo apt-get install -y libgdal-dev gdal-bin
  fi
}

install_alpine() {
  info "Installing GDAL via apk…"
  apk add --no-cache gdal-dev
}

install_macos() {
  info "Installing GDAL via Homebrew…"
  brew install gdal
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

ok "GDAL installed successfully ($(gdal-config --version 2>/dev/null || echo 'version unknown'))"
