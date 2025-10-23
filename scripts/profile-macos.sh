#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# also: alloc, sys, time
#cargo instruments -t "CPU Profiler" --bin versatiles -- convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 https://download.versatiles.org/osm.versatiles ./tmp/temp.versatiles
#versatiles convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 https://download.versatiles.org/osm.versatiles ./tmp/temp.versatiles
#versatiles convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 http://localhost:8080/osm.versatiles ./tmp/temp.versatiles
#cargo instruments -t "CPU Profiler" --package versatiles --bin versatiles --all-features -- convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 http://localhost:8080/osm.versatiles ./tmp/temp.versatiles
#cargo instruments -t "CPU Profiler" --package versatiles --bin versatiles -- convert --bbox 5,46,7,48 ../../temp/world.pmtiles ../../temp/world.versatiles

#source scripts/env-gdal.sh

#cargo instruments -t "CPU Profiler" --bin versatiles --features gdal,bindgen --release -- convert ../../temp/paris.vpl ../../temp/paris.versatiles
#cargo instruments -t "Allocations" --bin versatiles --features gdal,bindgen --time-limit 300000 -- convert ../../temp/paris.vpl ../../temp/paris.versatiles
cargo instruments -t "CPU Profiler" --bin versatiles -- convert -c gzip --max-zoom 8 ../../temp/remove-labels.vpl ../../temp/temp.versatiles