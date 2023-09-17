#!/usr/bin/env bash

set -e

FOLDER="$1"
FILENAME="versatiles-$2"
TAG=$3

cd "$FOLDER"
tar -cf "$FILENAME.tar" "versatiles"
gzip -9 "$FILENAME.tar"
sha256sum "$FILENAME.tar.gz" | sed 's/ .*//' >"$FILENAME.tar.gz.sha256"
md5sum "$FILENAME.tar.gz" | sed 's/ .*//' >"$FILENAME.tar.gz.md5"
gh release upload "$TAG" $FILENAME.tar.gz* --clobber
