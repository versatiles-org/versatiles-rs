#!/bin/sh

# `-u` as well as `-e`: this script runs as root via `curl | sudo sh`, and an
# unset variable expanding to nothing is how a path like "/usr/local/bin/$X"
# quietly becomes something else. (`pipefail` is not POSIX, so it is not
# available here — which is part of why the download below is no longer a pipe.)
set -eu

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

# Download and install the package.
#
# The archive goes to a temporary file rather than straight down a pipe into
# `tar`: a stream cannot be checked before it is unpacked, and what this unpacks
# lands in /usr/local/bin as root. It is compared against the SHA-256 published
# beside it and only then extracted. A missing or mismatched checksum aborts —
# the point is to refuse an archive we cannot account for, so there is no flag
# to skip this.
PACKAGE_URL="https://github.com/versatiles-org/versatiles-rs/releases/latest/download/versatiles-$OS-$ARCH.tar.gz"
CHECKSUM_URL="$PACKAGE_URL.sha256"

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT HUP INT TERM

# `-f` / the wget default make an HTTP error an error: without it a 404 page is
# saved as the archive and the failure only surfaces later, as a corrupt file.
download() {
   if command -v curl >/dev/null 2>&1; then
      curl -fLsS "$1" -o "$2"
   elif command -v wget >/dev/null 2>&1; then
      wget -q "$1" -O "$2"
   else
      echo "Error: Neither curl nor wget is installed." >&2
      exit 1
   fi
}

echo "Downloading $PACKAGE_URL"
download "$PACKAGE_URL" "$WORKDIR/versatiles.tar.gz"
download "$CHECKSUM_URL" "$WORKDIR/versatiles.tar.gz.sha256"

# The published file is "<hash>  <filename>"; compare the hash only, since the
# filename in it is the release asset's, not the local temporary one.
EXPECTED=$(cut -d' ' -f1 <"$WORKDIR/versatiles.tar.gz.sha256")
if command -v sha256sum >/dev/null 2>&1; then
   ACTUAL=$(sha256sum "$WORKDIR/versatiles.tar.gz" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then
   ACTUAL=$(shasum -a 256 "$WORKDIR/versatiles.tar.gz" | cut -d' ' -f1)
else
   echo "Error: neither sha256sum nor shasum is available to verify the download." >&2
   exit 1
fi

if [ -z "$EXPECTED" ] || [ "$EXPECTED" != "$ACTUAL" ]; then
   echo "Error: checksum mismatch for $PACKAGE_URL" >&2
   echo "  expected: $EXPECTED" >&2
   echo "  actual:   $ACTUAL" >&2
   exit 1
fi
echo "Checksum verified."

tar -xzf "$WORKDIR/versatiles.tar.gz" -C /usr/local/bin versatiles

echo "VersaTiles installed successfully."
