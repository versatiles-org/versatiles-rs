#!/usr/bin/env bash
cd "$(dirname "$0")/.."

# also: alloc, sys, time
#cargo instruments -t "CPU Profiler" --bin versatiles -- convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 https://download.versatiles.org/osm.versatiles ./tmp/temp.versatiles
#versatiles convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 https://download.versatiles.org/osm.versatiles ./tmp/temp.versatiles
#versatiles convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 http://localhost:8080/osm.versatiles ./tmp/temp.versatiles
#cargo instruments -t "CPU Profiler" --package versatiles --bin versatiles --all-features -- convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 http://localhost:8080/osm.versatiles ./tmp/temp.versatiles
#cargo instruments -t "CPU Profiler" --package versatiles --bin versatiles -- convert --bbox 5,46,7,48 ../../temp/world.pmtiles ../../temp/world.versatiles

#source scripts/env-gdal.sh

#cargo instruments -t "CPU Profiler" --bin versatiles --features gdal --release -- convert ../../temp/paris.vpl ../../temp/paris.versatiles
#cargo instruments -t "Allocations" --bin versatiles --features gdal --time-limit 300000 -- convert ../../temp/paris.vpl ../../temp/paris.versatiles
#cargo instruments -t "CPU Profiler" --bin versatiles -- convert -c gzip --max-zoom 8 ../../temp/remove-labels.vpl ../../temp/temp.versatiles

# profile a specific test
#rm -r ./target/debug/deps/versatiles_pipeline-????????????????
#rm -f test.trace
#cargo test -p versatiles_pipeline --no-run
#xcrun xctrace record \
#  --template 'Time Profiler' \
#  --output test.trace \
#  --launch -- \
#  ./target/debug/deps/versatiles_pipeline-???????????????? \
#  container_reader::reader::tests::open_vpl_str \
#  --exact --test-threads=1

cargo instruments -t "CPU Profiler" --bin versatiles -- convert -b 11,51,15,54 --min-zoom 10 --max-zoom 14 ../../tiles/satellite/satellite.vpl ../../tiles/satellite/result.versatiles
