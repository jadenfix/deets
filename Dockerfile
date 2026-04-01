# Aether Blockchain - Multi-stage Docker Build
FROM rust:1.86-slim as builder

WORKDIR /build

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    clang \
    llvm-dev \
    libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY ai-mesh ./ai-mesh

# Build release binary
RUN cargo build --release --bin aether-node

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary
COPY --from=builder /build/target/release/aether-node /usr/local/bin/

# Copy config
COPY config/genesis.toml /app/config/

# Create data directory
RUN mkdir -p /app/data

ENV AETHER_CONFIG_PATH=/app/config/genesis.toml
ENV AETHER_NODE_DB_PATH=/app/data
ENV AETHER_RPC_BIND=0.0.0.0

EXPOSE 8545 9000

ENTRYPOINT ["aether-node"]
