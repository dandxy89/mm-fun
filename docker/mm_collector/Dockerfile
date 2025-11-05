# Multi-stage build for mm_collector
FROM rust:1.91.0-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    build-essential \
    uuid-dev \
    libbsd-dev \
    libclang-dev \
    clang \
    && rm -rf /var/lib/apt/lists/*

# Install rustfmt for bindgen
RUN rustup component add rustfmt

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY mm_binary ./mm_binary
COPY mm_types ./mm_types
COPY mm_orderbook ./mm_orderbook
COPY mm_ws ./mm_ws
COPY mm_http ./mm_http
COPY mm_aeron ./mm_aeron
COPY mm_tg ./mm_tg
COPY mm_app ./mm_app
COPY mm_ratelimit ./mm_ratelimit
COPY mm_strategy ./mm_strategy
COPY mm_sim_executor ./mm_sim_executor
COPY mm_backtest ./mm_backtest

# Build release binary
RUN cargo build --release --bin mm_collector

# Runtime stage - using Debian trixie for GLIBC 2.38+ support
FROM debian:trixie-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    libuuid1 \
    libbsd0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/mm_collector /app/

# Copy Aeron shared libraries from builder
COPY --from=builder /build/target/release/build/rusteron-client-*/out/build/lib/*.so* /usr/local/lib/
COPY --from=builder /build/target/release/build/rusteron-media-driver-*/out/build/lib/*.so* /usr/local/lib/
RUN ldconfig

# Create logs directory
RUN mkdir -p /app/logs

# Set environment
ENV RUST_LOG=info
ENV AERON_DIR=/dev/shm/aeron

# Run the collector
CMD ["/app/mm_collector"]
