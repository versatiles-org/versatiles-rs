# get ARGs
ARG ARCH
ARG LIBC



# CREATE BUILDER SYSTEM MUSL
FROM --platform=${TARGETPLATFORM} alpine:latest as builder_musl
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN apk add bash curl gcc musl-dev openssl-dev pkgconfig sqlite-dev



# CREATE BUILDER SYSTEM GNU
FROM --platform=${TARGETPLATFORM} debian:latest as builder_gnu
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update && \
    apt install -y build-essential curl libsqlite3-dev libssl-dev pkg-config



# CREATE FINAL BUILDER SYSTEM RUST
FROM builder_${LIBC} as builder
ARG ARCH
ARG LIBC

# install rust, test, build and test again
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
ENV TARGET $ARCH-unknown-linux-$LIBC
ENV PATH="/root/.cargo/bin:$PATH"
RUN rustup target add "$TARGET"

WORKDIR /versatiles
COPY . .
RUN cargo test --all-features --target "$TARGET" --release --bin "versatiles"
RUN cargo build --all-features --target "$TARGET" --release --bin "versatiles"
RUN ./helpers/versatiles_selftest.sh "./target/$TARGET/release/versatiles"



# EXTRACT RESULT
FROM scratch
ARG ARCH
ARG LIBC
COPY --from=builder "/versatiles/target/$ARCH-unknown-linux-$LIBC/release/versatiles" /versatiles
