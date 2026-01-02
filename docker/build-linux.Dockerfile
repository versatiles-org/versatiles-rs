# Get ARGs
ARG LIBC

# CREATE BUILDER SYSTEM FOR MUSL
FROM rust:alpine AS builder_musl
# NOTE: We use +crt-static for CLI binaries (fully static)
# but -crt-static for cdylib (Node.js bindings) to enable dynamic linking
# The RUSTFLAGS will be overridden per-build below
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
ARG RUN_TESTS=true

# Set the target architecture
ENV TARGET="${ARCH}-unknown-linux-${LIBC}"
# Add Rust target
RUN rustup target add "$TARGET"

# Set working directory
WORKDIR /versatiles

# Copy the source code
COPY . .

# Run tests, build the project, and run self-tests
# Note: Tests can be skipped for cross-compilation builds to avoid OOM
# Set RUN_TESTS=false to skip (native platform tests run separately in CI)
RUN if [ "$RUN_TESTS" = "true" ]; then \
        echo "Running tests for $TARGET..."; \
        cargo test --target "$TARGET"; \
    else \
        echo "Skipping tests (RUN_TESTS=false)"; \
    fi
# Build CLI with static linking for musl (fully static binary)
RUN if [ "$LIBC" = "musl" ]; then \
        RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build --package "versatiles" --bin "versatiles" --release --target "$TARGET"; \
    else \
        cargo build --package "versatiles" --bin "versatiles" --release --target "$TARGET"; \
    fi
RUN ./scripts/selftest-versatiles.sh "/versatiles/target/$TARGET/release/versatiles"

# Build NAPI binding for Node.js
# For musl: use dynamic linking to enable cdylib (-C target-feature=-crt-static)
# For gnu: use default flags
RUN if [ "$LIBC" = "musl" ]; then \
        RUSTFLAGS="-C target-feature=-crt-static" \
        cargo build --package "versatiles_node" --release --target "$TARGET"; \
    else \
        cargo build --package "versatiles_node" --release --target "$TARGET"; \
    fi

# Prepare output directory
RUN mkdir -p /output/cli /output/node && \
    cp "/versatiles/target/$TARGET/release/versatiles" /output/cli/ && \
    cp "/versatiles/target/$TARGET/release/libversatiles_node.so" /output/node/

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

