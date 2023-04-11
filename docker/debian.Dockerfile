# create builder system
FROM debian:stable as builder

# install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update
RUN apt install -y build-essential curl libsqlite3-dev libssl-dev pkg-config

# install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable

# install versatiles
RUN $HOME/.cargo/bin/cargo install versatiles

# create production system
FROM debian:stable-slim
WORKDIR /data/

# install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update && \
    apt install -y --no-install-recommends curl libsqlite3-0 && \
    apt clean && \
    apt autoremove --yes && \
    rm -rf /var/lib/apt/lists/* && \
    rm -rf /var/cache/*

# copy versatiles and tests
COPY --from=builder /root/.cargo/bin/versatiles /usr/bin/
COPY helpers/versatiles_selftest.sh .
