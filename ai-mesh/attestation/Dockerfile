# ============================================================================
# AETHER AI WORKER - SEV-SNP Attested Container
# ============================================================================
# PURPOSE: Reproducible container for TEE-attested AI inference
#
# BASE: Ubuntu 22.04 with SEV-SNP support
# COMPONENTS:
# - AI runtime (Rust binary)
# - Model inference engine (ONNX Runtime)
# - TEE attestation library
# - KZG proof generation
#
# BUILD:
#   docker build -t aether-ai-worker:latest -f Dockerfile .
#
# RUN:
#   # Inside SEV-SNP VM
#   docker run --device=/dev/sev-guest aether-ai-worker:latest
# ============================================================================

FROM ubuntu:22.04

# Reproducible build settings
ENV DEBIAN_FRONTEND=noninteractive
ENV TZ=UTC

# Install dependencies (pinned versions)
RUN apt-get update && apt-get install -y \
    curl=7.81.0-1ubuntu1.15 \
    ca-certificates=20230311ubuntu0.22.04.1 \
    && rm -rf /var/lib/apt/lists/*

# Install Rust toolchain (pinned version)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --default-toolchain 1.75.0

# Copy AI runtime binary
COPY target/release/aether-ai-runtime /usr/local/bin/

# Copy models
COPY ai-mesh/models/ /opt/aether/models/

# TEE device access
VOLUME /dev/sev-guest

# Expose metrics
EXPOSE 9090

# Entry point
ENTRYPOINT ["/usr/local/bin/aether-ai-runtime"]

