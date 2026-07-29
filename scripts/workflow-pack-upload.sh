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

# Retry uploads: `gh release upload` makes a single call to api.github.com and
# transient connectivity blips on the runner would otherwise fail the whole release.
upload() {
   local attempt
   for attempt in 1 2 3 4 5; do
      if gh release upload "$@"; then
         return 0
      fi
      echo "upload attempt $attempt failed; retrying in $((attempt * 10))s..." >&2
      sleep $((attempt * 10))
   done
   echo "upload failed after 5 attempts: gh release upload $*" >&2
   return 1
}

upload "$TAG" $FILENAME.tar.gz* --clobber

if ls *.deb 1>/dev/null  2>&1; then
   upload "$TAG" *.deb --clobber
fi
