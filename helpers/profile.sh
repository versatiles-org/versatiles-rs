#!/usr/bin/env bash

cd "$(dirname "$0")/.."

# also: alloc, sys, time
#cargo instruments -t "CPU Profiler" --bin versatiles -- convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 https://download.versatiles.org/planet-latest.versatiles ./tmp/temp.versatiles
#versatiles convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 https://download.versatiles.org/planet-latest.versatiles ./tmp/temp.versatiles
#versatiles convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 http://localhost:8080/planet-latest.versatiles ./tmp/temp.versatiles
cargo instruments -t "CPU Profiler" --bin versatiles -- convert --bbox 5.63,48.93,11.24,45.08 --min-zoom 14 http://localhost:8080/planet-latest.versatiles ./tmp/temp.versatiles
