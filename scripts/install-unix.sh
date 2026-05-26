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

# Detect OS and pick libc variant
# On Linux, default to the fully-static musl build — it runs on any kernel
# regardless of the host's glibc version, avoiding "GLIBC_x.y not found"
# failures on older distros. Set VERSATILES_LIBC=gnu to opt into the
# dynamically-linked glibc build instead.
OS=$(uname)
case $OS in
   Linux)
      LIBC="${VERSATILES_LIBC:-musl}"
      case $LIBC in
         musl|gnu) OS="linux-$LIBC" ;;
         *) echo "Unsupported VERSATILES_LIBC: $LIBC (expected musl or gnu)"; exit 1 ;;
      esac
      ;;
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
fi | tar -xzf - -C /usr/local/bin versatiles

echo "VersaTiles installed successfully."
