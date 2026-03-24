#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    -h|--help)
      echo "Usage: $(basename "$0") [--install]"
      echo "  --install  Copy the binary to /usr/local/bin after building"
      exit 0 ;;
    *) echo "Unknown argument: $arg" >&2; exit 1 ;;
  esac
done

cargo build -F gdal --release

if [ "$INSTALL" = 1 ]; then
  BINARY="target/release/versatiles"
  DEST="/usr/local/bin/versatiles"

  if [ ! -f "$BINARY" ]; then
    echo "Error: binary not found at $BINARY" >&2
    exit 1
  fi

  if [ -w "$(dirname "$DEST")" ]; then
    cp "$BINARY" "$DEST"
  else
    sudo cp "$BINARY" "$DEST"
  fi

  echo "Installed to $DEST"
fi
