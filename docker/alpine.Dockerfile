FROM alpine as builder

RUN apk add musl-dev curl gcc openssl-dev pkgconfig sqlite-dev
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
RUN $HOME/.cargo/bin/cargo install versatiles

FROM alpine

RUN apk add --no-cache curl

COPY --from=builder /root/.cargo/bin/versatiles /usr/bin/
COPY --from=builder /usr/lib/libsqlite3.so.0 /usr/lib/
COPY --from=builder /usr/lib/libgcc_s.so.1 /usr/lib/
