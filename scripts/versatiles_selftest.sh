#!/usr/bin/env bash
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
