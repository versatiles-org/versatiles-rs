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
RUN cargo test --target "$TARGET"
RUN cargo build --package "versatiles" --bin "versatiles" --release --target "$TARGET"
RUN ./scripts/selftest-versatiles.sh "/versatiles/target/$TARGET/release/versatiles"

# Build NAPI binding for Node.js (only for GNU, musl doesn't support cdylib)
RUN if [ "$LIBC" = "gnu" ]; then \
    cargo build --package "versatiles_node" --release --target "$TARGET"; \
fi

# Prepare output directory
RUN mkdir -p /output/cli /output/node && \
    cp "/versatiles/target/$TARGET/release/versatiles" /output/cli/ && \
    if [ "$LIBC" = "gnu" ]; then \
        cp "/versatiles/target/$TARGET/release/libversatiles_node.so" /output/node/; \
    fi

# Build .deb package if using GNU
RUN if [ "$LIBC" = "gnu" ]; then \
    cargo install cargo-deb && \
    cargo deb --no-build --target "$TARGET" --package "versatiles" --output "/output/cli/versatiles-linux-${LIBC}-${ARCH}.deb"; \
fi

# FINAL STAGE TO EXTRACT RESULT
FROM scratch
ARG ARCH
ARG LIBC

# Copy the compiled binaries and package from the builder
COPY --from=builder /output /

