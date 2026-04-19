#!/usr/bin/env bash
# CI script: package a compiled binary as .tar.gz and upload it to a GitHub release.
#
# Usage:
#   ./scripts/workflow-pack-upload.sh <folder> <filename-stem> <tag>
#
# Compresses the "versatiles" binary from <folder>/cli into <filename-stem>.tar.gz,
# then uploads it (and any .deb files found) to the specified GitHub release tag.

set -e

FOLDER="$1"
FILENAME="versatiles-$2"
TAG=$3

cd "$FOLDER/cli"
tar -cf "$FILENAME.tar" "versatiles"
gzip -9 "$FILENAME.tar"

# case $(uname -s) in
#    Linux*)
#       sha256sum "$FILENAME.tar.gz" >"$FILENAME.tar.gz.sha256"
#       md5sum "$FILENAME.tar.gz" >"$FILENAME.tar.gz.md5"
#       ;;
#    Darwin*)
#       shasum -a 256 "$FILENAME.tar.gz" >"$FILENAME.tar.gz.sha256"
#       md5 "$FILENAME.tar.gz" >"$FILENAME.tar.gz.md5"
#       ;;
#    *)
#       echo "Unknown OS: $(uname -s)"
#       ;;
# esac

gh release upload "$TAG" $FILENAME.tar.gz* --clobber

if ls *.deb 1>/dev/null  2>&1; then
   gh release upload "$TAG" *.deb --clobber
fi
