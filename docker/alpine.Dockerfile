FROM rust:alpine

RUN set -eux; \
    apk add sqlite-dev curl gzip musl-dev; \
    cargo install versatiles; \
    rm -r /usr/local/cargo/registry; \
    rm -r /usr/local/rustup
