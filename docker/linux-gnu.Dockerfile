# create builder system
FROM --platform=$TARGETPLATFORM debian as builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM

# install dependencies
RUN apt update && \
    apt install -y build-essential curl libsqlite3-dev libssl-dev pkg-config

# install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
