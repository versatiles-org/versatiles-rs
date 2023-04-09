FROM debian:stable-slim as builder

RUN apt update
RUN apt -y install curl libssl-dev pkg-config libsqlite3-dev
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
RUN cargo install versatiles

FROM debian:stable-slim

RUN apt -y install curl

COPY --from=builder /root/.cargo/bin/versatiles /usr/bin/
COPY --from=builder /usr/lib/libsqlite3.so.0 /usr/lib/
COPY --from=builder /usr/lib/libgcc_s.so.1 /usr/lib/
