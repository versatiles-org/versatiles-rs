#!/usr/bin/env bash

cd "$(dirname "$0")/.."

# also: alloc, sys, time
cargo instruments -t "CPU Profiler" --bin versatiles -- convert --bbox 0,0,5,5 --max-zoom 14 https://download.versatiles.org/planet-latest.versatiles ./tmp/temp.versatiles
