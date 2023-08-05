# create builder system
FROM --platform=arm64 alpine:latest as builder

# install dependencies
RUN apk add curl gcc musl-dev openssl-dev pkgconfig sqlite-dev

# install rust
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:$PATH"
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN rustup target add aarch64-unknown-linux-musl

# tests here
WORKDIR /versatiles
COPY . .
RUN cargo test --all-features --target aarch64-unknown-linux-musl --release --bin versatiles
RUN cargo build --all-features --target aarch64-unknown-linux-musl --release --bin versatiles
RUN find .

FROM scratch
COPY --from=builder /versatiles/target/aarch64-unknown-linux-musl/release/versatiles /versatiles
