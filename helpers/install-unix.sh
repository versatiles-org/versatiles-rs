#!/bin/sh

set -e

# Detect architecture

ARCH=$(uname -m)
case $ARCH in
   x86_64) ARCH="x86_64" ;;
   aarch64 | arm64 | aarch64_be | armv8b | armv8l) ARCH="aarch64" ;;
   *)
      echo "Unsupported architecture: $ARCH"
      exit 1
      ;;
esac
echo "Detected architecture: $ARCH"

# Detect OS and libc type
OS=$(uname)
case $OS in
   Linux) OS="linux-$(ldd --version 2>&1 | grep -q 'musl' && echo 'musl' || echo 'gnu')" ;;
   Darwin) OS="macos" ;;
   *)
      echo "Unsupported OS: $OS"
      exit 1
      ;;
esac
echo "Detected OS: $OS"

# Download and install the package
PACKAGE_URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-$OS-$ARCH.tar.gz"
if command -v curl >/dev/null 2>&1; then
   curl -Ls "$PACKAGE_URL"
elif command -v wget >/dev/null 2>&1; then
   wget -qO- "$PACKAGE_URL"
else
   echo "Error: Neither curl nor wget is installed." >&2
   exit 1
fi | sudo tar -xzf - -C /usr/local/bin versatiles

echo "VersaTiles installed successfully."
