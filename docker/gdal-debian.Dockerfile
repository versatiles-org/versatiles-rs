# Pinned by digest. `FROM debian` is `debian:latest`, which is whatever Debian
# published most recently — the base changes without the file changing.
FROM debian:latest@sha256:9cc080028c43b27d2074d63a5f9caf7166d731494965616c1a6d2827a004585c
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -y
COPY scripts/install-gdal.sh scripts/install-gdal.sh
RUN ./scripts/install-gdal.sh
RUN apt-get clean && rm -rf /var/lib/apt/lists/*
COPY . .
