#!/usr/bin/env bash
# CI script: package a compiled binary as .tar.gz and upload it to a GitHub release.
#
# Usage:
#   ./scripts/workflow-pack-upload.sh <folder> <filename-stem> <tag>
#
# Compresses the "versatiles" binary from <folder>/cli into <filename-stem>.tar.gz,
# then uploads it (and any .deb files found) to the specified GitHub release tag.

set -euo pipefail

FOLDER="$1"
FILENAME="versatiles-$2"
TAG="$3"

cd "$FOLDER/cli"
tar -cf "$FILENAME.tar" "versatiles"
gzip -9 "$FILENAME.tar"

# Publish a SHA-256 next to the tarball. The install scripts refuse to unpack an
# archive they cannot check against this file, and the Homebrew formula needs a
# digest it did not compute itself from the same download — so a release without
# one is a release nobody can verify.
#
# Written in coreutils format ("<hash>  <filename>") so `sha256sum -c` accepts
# it directly. No MD5: publishing a broken digest beside a good one only invites
# somebody to check the wrong one.
case "$(uname -s)" in
   Linux*)
      sha256sum "$FILENAME.tar.gz" >"$FILENAME.tar.gz.sha256"
      ;;
   Darwin*)
      shasum -a 256 "$FILENAME.tar.gz" >"$FILENAME.tar.gz.sha256"
      ;;
   *)
      # Fail rather than skip: silently shipping an unverifiable artifact is
      # what this whole block exists to prevent.
      echo "cannot compute a checksum on $(uname -s)" >&2
      exit 1
      ;;
esac

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

upload "$TAG" "$FILENAME.tar.gz" "$FILENAME.tar.gz.sha256" --clobber

if ls ./*.deb 1>/dev/null 2>&1; then
   upload "$TAG" ./*.deb --clobber
fi
