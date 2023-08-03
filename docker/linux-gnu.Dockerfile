# create builder system
FROM --platform=$TARGETPLATFORM debian as builder

ARG TARGETPLATFORM
ARG BUILDPLATFORM

# install dependencies
RUN apk add curl gcc musl-dev openssl-dev pkgconfig sqlite-dev

# install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
