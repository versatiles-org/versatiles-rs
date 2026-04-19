#!/usr/bin/env bash
# Load-test a local tile server with parallel HTTP requests.
#
# Sends 300 tile requests (10 in parallel) to a server running on
# localhost:8080 and reports total elapsed time. Requires GNU parallel.
# Start the server separately before running this script.

# parallel -j 1 --progress "curl -s 'http://localhost:8080/tiles/osm/14/8192/{}' | wc -c" ::: {5700..10000}
time parallel -j 10 --progress "curl -s -H 'accept-encoding:br' 'http://localhost:8080/tiles/osm/14/8192/{}' > /dev/null" ::: {5700..6000}
