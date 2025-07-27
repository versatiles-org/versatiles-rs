#!/usr/bin/env bash
cd "$(dirname "$0")/.."

docker build -t versatiles-gdal --target runner --file docker/gdal-debian.Dockerfile .

# docker run -it --rm -p 8080:8080 --volume .:/data versatiles-gdal serve -s frontend-dev.br.tar world.vpl

# time docker run -it --rm --volume .:/data versatiles-gdal convert world.vpl temp.versatiles --max-zoom 3


# docker run -it --rm --volume .:/data --entrypoint "/usr/bin/gdaladdo" versatiles-gdal world.overview.tif

