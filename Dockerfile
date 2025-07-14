# syntax=docker/dockerfile:1.6

###############################
# 1️⃣ Build stage
###############################
FROM ghcr.io/osgeo/gdal:ubuntu-full-3.10.3 AS builder

# Environment variables so Rust installs in a predictable path that we can cache
ENV RUSTUP_HOME=/usr/local/rustup \
	CARGO_HOME=/usr/local/cargo \
	PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        build-essential curl clang llvm-dev libclang-dev pkg-config ca-certificates && \
    curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /app

# ── Source layer & build ────────────────────────────────────────────────
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
	--mount=type=cache,target=/app/target \
	cargo build --release --features gdal && \
	cp /app/target/release/versatiles / && \
	strip /versatiles

###############################
# 2️⃣ Runtime stage
###############################
FROM ghcr.io/osgeo/gdal:ubuntu-full-3.10.3 AS runtime

WORKDIR /data

# Copy the statically linked binary from the builder
COPY --from=builder /versatiles /usr/local/bin/versatiles

ENTRYPOINT ["versatiles"]
