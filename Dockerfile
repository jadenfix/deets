# Aether Blockchain - Multi-stage Docker Build
FROM rust:1.80-slim as builder

WORKDIR /build

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
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
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary
COPY --from=builder /build/target/release/aether-node /usr/local/bin/

# Copy config
COPY config/genesis.toml /app/config/

# Create data directory
RUN mkdir -p /app/data

EXPOSE 8545 9000

ENTRYPOINT ["aether-node"]
CMD ["--config", "/app/config/genesis.toml", "--data-dir", "/app/data"]

