# get ARGs
ARG LIBC



# CREATE BUILDER SYSTEM MUSL
FROM --platform=${TARGETPLATFORM} rust:alpine as builder_musl
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN apk add bash musl-dev



# CREATE BUILDER SYSTEM GNU
FROM --platform=${TARGETPLATFORM} rust:latest as builder_gnu
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update -y && apt install -y bash



# CREATE FINAL BUILDER SYSTEM RUST
FROM builder_${LIBC} as builder
ARG ARCH
ARG LIBC

# set target, then test, build and test again
ENV TARGET $ARCH-unknown-linux-$LIBC
ENV PATH="/root/.cargo/bin:$PATH"
RUN rustup target add "$TARGET"
WORKDIR /versatiles
COPY . .
RUN cargo test --all-features --target "$TARGET" --release --bin "versatiles"
RUN cargo build --all-features --target "$TARGET" --release --bin "versatiles"
RUN ./helpers/versatiles_selftest.sh "/versatiles/target/$TARGET/release/versatiles"



# EXTRACT RESULT
FROM scratch
ARG ARCH
ARG LIBC
COPY --from=builder "/versatiles/target/$ARCH-unknown-linux-$LIBC/release/versatiles" /versatiles
