#!/usr/bin/env bash
cd "$(dirname "$0")/.."

docker build -t versatiles-gdal --file docker/gdal-debian.Dockerfile .
