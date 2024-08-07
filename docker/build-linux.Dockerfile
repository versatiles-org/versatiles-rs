# Get ARGs
ARG LIBC

# CREATE BUILDER SYSTEM FOR MUSL
FROM rust:alpine AS builder_musl
# Enable static linking
ENV RUSTFLAGS="-C target-feature=+crt-static"
# Install necessary packages
RUN apk add --no-cache bash musl-dev

# CREATE BUILDER SYSTEM FOR GNU
FROM rust:slim AS builder_gnu
# Avoid prompts during package installation
ENV DEBIAN_FRONTEND=noninteractive
# Install necessary packages
RUN apt update -y && apt install -y bash && rm -rf /var/lib/apt/lists/*

# SELECT BUILDER BASED ON LIBC
FROM builder_${LIBC} AS builder

# Set up build arguments
ARG ARCH
ARG LIBC

# Set the target architecture
ENV TARGET="${ARCH}-unknown-linux-${LIBC}"
# Add Rust target
RUN rustup target add "$TARGET"

# Set working directory
WORKDIR /versatiles

# Copy the source code
COPY . .

# Run tests, build the project, and run self-tests
RUN cargo test --all-features --target "$TARGET"
RUN cargo build --all-features --package "versatiles" --bin "versatiles" --release --target "$TARGET"
RUN ./helpers/versatiles_selftest.sh "/versatiles/target/$TARGET/release/versatiles"

# Prepare output directory
RUN mkdir /output && cp "/versatiles/target/$TARGET/release/versatiles" /output/

# Build .deb package if using GNU
RUN if [ "$LIBC" = "gnu" ]; then \
    cargo install cargo-deb && \
    cargo deb --no-build --target "$TARGET" --package "versatiles" --output "/output/versatiles-linux-${LIBC}-${ARCH}.deb"; \
fi

# FINAL STAGE TO EXTRACT RESULT
FROM scratch
ARG ARCH
ARG LIBC

# Copy the compiled binary and package from the builder
COPY --from=builder /output /

