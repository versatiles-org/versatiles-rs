FROM rust:slim-bullseye

RUN set -eux; \
    apt-get update; \
    apt-get -y install libsqlite3-dev curl gzip; \
    cargo install versatiles; \
    rm -r /usr/local/cargo/registry; \
    rm -r /usr/local/rustup
