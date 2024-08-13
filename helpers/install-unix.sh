#!/bin/bash

# Function to detect the system architecture
detect_architecture() {
   ARCH=$(uname -m)
   case $ARCH in
      x86_64)
         ARCH="x86_64"
         ;;
      arm64 | aarch64)
         ARCH="aarch64"
         ;;
      *)
         echo "Unsupported architecture: $ARCH"
         exit 1
         ;;
   esac
}

# Function to detect the system OS and libc type
detect_os() {
   case "$(uname)" in
      Linux)
         if ldd --version 2>&1 | grep -q "musl"; then
            OS="linux-musl"
         else
            OS="linux-gnu"
         fi
         ;;
      Darwin)
         OS="macos"
         ;;
      *)
         echo "Unsupported OS: $(uname)"
         exit 1
         ;;
   esac
}

# Function to download and install the package
install_package() {
   PACKAGE_URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-$OS-$ARCH.tar.gz"

   wget -q -O versatiles.tar.gz "$PACKAGE_URL"
   if [ $? -ne 0 ]; then
      echo "Failed to download the tarball"
      exit 1
   fi

   tar -xzf versatiles.tar.gz
   sudo mv versatiles /usr/local/bin/
   rm -f versatiles.tar.gz

   echo "Versatiles installed successfully."
}

# Main script execution
detect_architecture
detect_os
install_package
