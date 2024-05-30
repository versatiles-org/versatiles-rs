#!/bin/bash

set -e

# Determine the architecture and libc type
ARCH=$(uname -m)
LIBC=$(ldd --version 2>&1 | head -n 1 | tr '[:upper:]' '[:lower:]' | grep -o 'musl\|glibc')

# Map architecture to the correct download URL
case $ARCH in
  aarch64)
    if [ "$LIBC" == "musl" ]; then
      URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-linux-musl-aarch64.tar.gz"
    else
      URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-linux-gnu-aarch64.tar.gz"
    fi
    ;;
  x86_64)
    if [ "$LIBC" == "musl" ]; then
      URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-linux-musl-x86_64.tar.gz"
    else
      URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-linux-gnu-x86_64.tar.gz"
    fi
    ;;
  *)
    echo "Unsupported architecture: $ARCH"
    exit 1
    ;;
esac

# Download the tarball and extract the binary directly to /usr/local/bin/
echo "Downloading and extracting $URL..."
curl -Ls "$URL" | sudo tar -xzf - -C /usr/local/bin versatiles

# Set execute permissions for the binary
sudo chmod +x /usr/local/bin/versatiles

echo "Installation complete!"
