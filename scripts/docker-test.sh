#!/bin/bash
# Aether Blockchain - Docker Test Script

set -e

echo "Building test environment..."
docker compose -f docker-compose.test.yml build

echo "Starting 4-node test network..."
docker compose -f docker-compose.test.yml up -d validator-1 validator-2 validator-3 validator-4

echo "Waiting for network to initialize..."
sleep 15

echo "Running integration tests..."
docker compose -f docker-compose.test.yml run test-runner

echo "Stopping test network..."
docker compose -f docker-compose.test.yml down

echo "Docker tests completed!"
