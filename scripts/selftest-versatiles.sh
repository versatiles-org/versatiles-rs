#!/usr/bin/env bash
# Smoke-test the versatiles binary with a convert and serve command.
#
# Usage:
#   ./scripts/selftest-versatiles.sh [path-to-versatiles-binary]
#
# Defaults to the "versatiles" binary on PATH. Used inside Docker image
# builds to verify the binary works in the target environment.

dir=$(dirname $(dirname "$0"))
echo dir=$dir

set -e

cmd=$1
if [ -z ${cmd+x} ]; then
	cmd="versatiles"
fi

set -x

$cmd convert --max-zoom 3 "$dir/testdata/berlin.mbtiles" "$dir/testdata/test.versatiles"
$cmd serve --auto-shutdown 1000 -p 8088 "https://download.versatiles.org/osm.versatiles"
