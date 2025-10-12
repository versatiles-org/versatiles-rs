FROM debian
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update -y
COPY scripts/*.sh .
RUN ./scripts/install-gdal.sh
RUN apt clean && rm -rf /var/lib/apt/lists/*
COPY . .
