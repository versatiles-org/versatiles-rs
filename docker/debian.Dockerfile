FROM debian:testing as builder

RUN apt update
RUN apt -y install build-essential curl libsqlite3-dev libssl-dev pkg-config
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
RUN $HOME/.cargo/bin/cargo install versatiles

FROM debian:testing-slim

COPY --from=builder /root/.cargo/bin/versatiles /usr/bin/
#COPY --from=builder /usr/lib/libsqlite3.so.0 /usr/lib/
#COPY --from=builder /usr/lib/libgcc_s.so.1 /usr/lib/
