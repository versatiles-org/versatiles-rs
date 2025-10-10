FROM debian
ENV DEBIAN_FRONTEND=noninteractive
COPY . .
RUN apt update -y
# RUN apt install -y git curl build-essential cmake
RUN ./scripts/install-gdal.sh
RUN apt clean && rm -rf /var/lib/apt/lists/*

