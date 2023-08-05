# create builder system
FROM --platform=arm64 debian:latest as builder

# install dependencies
ENV DEBIAN_FRONTEND=noninteractive
RUN apt update && \
    apt install -y build-essential curl libsqlite3-dev libssl-dev pkg-config

# install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:$PATH"
RUN rustup target add aarch64-unknown-linux-gnu

# tests here
WORKDIR /versatiles
COPY Cargo.* .
COPY src src
RUN cargo test --all-features --target aarch64-unknown-linux-gnu --release --bin versatiles
RUN cargo build --all-features --target aarch64-unknown-linux-gnu --release --bin versatiles
RUN find .

FROM scratch
COPY --from=builder /versatiles/target/aarch64-unknown-linux-gnu/release/versatiles /versatiles
