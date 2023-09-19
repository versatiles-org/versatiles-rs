#!/usr/bin/env bash

set -e

FOLDER="$1"
FILENAME="versatiles-$2"
TAG=$3

cd "$FOLDER"
tar -cf "$FILENAME.tar" "versatiles"
gzip -9 "$FILENAME.tar"

case $(uname -s) in
   Linux*)
      sha256sum "$FILENAME.tar.gz" | sed 's/ .*//' >"$FILENAME.tar.gz.sha256"
      md5sum "$FILENAME.tar.gz" | sed 's/ .*//' >"$FILENAME.tar.gz.md5"
      ;;
   Darwin*)
      shasum -a 256 "$FILENAME.tar.gz" | sed 's/ .*//' >"$FILENAME.tar.gz.sha256"
      md5 -q "$FILENAME.tar.gz"  >"$FILENAME.tar.gz.md5"
      ;;
   *)
      echo "Unknown OS: $(uname -s)"
      ;;
esac

gh release upload "$TAG" $FILENAME.tar.gz* --clobber
