FROM debian
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -y
COPY scripts/install-gdal.sh scripts/install-gdal.sh
RUN ./scripts/install-gdal.sh
RUN apt-get clean && rm -rf /var/lib/apt/lists/*
COPY . .
