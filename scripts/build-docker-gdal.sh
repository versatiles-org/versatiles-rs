#!/usr/bin/env bash
# Build the GDAL-enabled Docker image from docker/gdal-debian.Dockerfile.
#
# Produces a local image tagged "versatiles-gdal" for testing GDAL builds
# inside a Debian container without installing GDAL on the host.

cd "$(dirname "$0")/.."

docker build -t versatiles-gdal --file docker/gdal-debian.Dockerfile .
