#!/bin/bash
# Aether Blockchain - Docker Test Script

set -e

echo "Building test environment..."
docker compose -f docker-compose.test.yml build

echo "Starting containerized node..."
docker compose -f docker-compose.test.yml up -d node

echo "Running integration tests..."
docker compose -f docker-compose.test.yml run --rm test-runner

echo "Stopping test network..."
docker compose -f docker-compose.test.yml down -v

echo "Docker tests completed!"
