# get ARGs
ARG arch
ARG libc



# CREATE BUILDER SYSTEM MUSL
FROM --platform=${TARGETPLATFORM} alpine:latest as builder_musl

# install dependencies
RUN apk add curl gcc musl-dev openssl-dev pkgconfig sqlite-dev
ENV RUSTFLAGS="-C target-feature=-crt-static"



# CREATE BUILDER SYSTEM GNU
FROM --platform=${TARGETPLATFORM} debian:latest as builder_gnu

# install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update && \
    apt install -y build-essential curl libsqlite3-dev libssl-dev pkg-config



# CREATE FINAL BUILDER SYSTEM RUST
FROM builder_${libc} as builder

# install rust, test, build and test again
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:$PATH"
RUN rustup target add "$arch-unknown-linux-$libc"

WORKDIR /versatiles
COPY . .
RUN cargo test --all-features --target "$arch-unknown-linux-$libc" --release --bin "versatiles"
RUN cargo build --all-features --target "$arch-unknown-linux-$libc" --release --bin "versatiles"
RUN ./helpers/versatiles_selftest.sh "./target/$arch-unknown-linux-$libc/release/versatiles"



# EXTRACT RESULT
FROM scratch
COPY --from=builder "./target/$arch-unknown-linux-$libc/release/versatiles" /versatiles
