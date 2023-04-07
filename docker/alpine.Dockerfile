FROM rust:alpine

RUN set -eux && \
    apk update && \
    apk add musl-dev openssl-dev pkgconfig sqlite-dev && \
    rustup default stable && \
    cargo install versatiles && \
    rm -r /usr/local/cargo/registry && \
    rm -r /usr/local/rustup
