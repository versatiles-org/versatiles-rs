# Compile Versatiles Binary inside Builder
FROM rust:alpine as builder

COPY ../ /usr/src/versatiles
WORKDIR /usr/src/versatiles

RUN apk add sqlite-dev curl gzip musl-dev git
RUN rustup default stable
RUN cargo install versatiles 
