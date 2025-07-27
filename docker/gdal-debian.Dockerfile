# syntax=docker/dockerfile:1

FROM debian:testing-slim AS builder

RUN apt-get update
RUN apt-get install -y gdal-bin libgdal-dev

FROM debian:testing-slim AS runner

USER root

COPY --from=builder  /build/usr/share/gdal/ /usr/share/gdal/

CMD ["/bin/bash", "-l"]
