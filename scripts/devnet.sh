#!/usr/bin/env bash
# ============================================================================
# Aether Devnet Launcher
# ============================================================================
# Launches a local 4-validator devnet for E2E testing.
#
# Usage:
#   ./scripts/devnet.sh          # Start devnet
#   ./scripts/devnet.sh stop     # Stop all nodes
#   ./scripts/devnet.sh clean    # Stop and remove data
#
# Each node gets:
#   - Unique RPC port (8545-8548)
#   - Unique P2P port (9000-9003)
#   - Unique data directory (./data/devnet/node{1-4})
#
# After starting, you can interact via JSON-RPC:
#   curl -s localhost:8545 -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}'
# ============================================================================

set -euo pipefail

NUM_VALIDATORS=4
BASE_RPC_PORT=8545
BASE_P2P_PORT=9000
DATA_DIR="./data/devnet"
PIDS_FILE="${DATA_DIR}/pids"
GENESIS_FILE="${DATA_DIR}/genesis.json"

stop_nodes() {
    if [ -f "$PIDS_FILE" ]; then
        echo "Stopping devnet nodes..."
        while read -r pid; do
            kill "$pid" 2>/dev/null || true
        done < "$PIDS_FILE"
        rm -f "$PIDS_FILE"
        echo "All nodes stopped."
    else
        echo "No running devnet found."
    fi
}

clean_data() {
    stop_nodes
    echo "Removing devnet data..."
    rm -rf "$DATA_DIR"
    echo "Clean."
}

if [ "${1:-}" = "stop" ]; then
    stop_nodes
    exit 0
fi

if [ "${1:-}" = "clean" ]; then
    clean_data
    exit 0
fi

# Build the node binary
echo "Building aether-node..."
cargo build --release -p aether-node 2>&1 | tail -1
NODE_BIN="./target/release/aether-node"

if [ ! -f "$NODE_BIN" ]; then
    echo "ERROR: Failed to build aether-node"
    exit 1
fi

# Create data directories
mkdir -p "$DATA_DIR"
for i in $(seq 1 $NUM_VALIDATORS); do
    mkdir -p "${DATA_DIR}/node${i}"
done

# Stop any existing nodes
stop_nodes

echo ""
echo "========================================="
echo "  Aether Devnet ($NUM_VALIDATORS validators)"
echo "========================================="
echo ""

# Launch nodes
> "$PIDS_FILE"

for i in $(seq 1 $NUM_VALIDATORS); do
    RPC_PORT=$((BASE_RPC_PORT + i - 1))
    P2P_PORT=$((BASE_P2P_PORT + i - 1))
    DB_PATH="${DATA_DIR}/node${i}"

    echo "Starting node $i (RPC: $RPC_PORT, P2P: $P2P_PORT)..."

    AETHER_NETWORK=devnet \
    AETHER_RPC_PORT=$RPC_PORT \
    AETHER_P2P_PORT=$P2P_PORT \
    AETHER_NODE_DB_PATH="$DB_PATH" \
    "$NODE_BIN" > "${DATA_DIR}/node${i}.log" 2>&1 &

    PID=$!
    echo "$PID" >> "$PIDS_FILE"
    echo "  Node $i started (PID: $PID)"

    # Brief delay so first node is listening before others try to connect
    sleep 0.5
done

echo ""
echo "Devnet running! Logs in ${DATA_DIR}/node*.log"
echo ""
echo "Quick test:"
echo "  curl -s localhost:${BASE_RPC_PORT} -X POST -H 'Content-Type: application/json' \\"
echo "    -d '{\"jsonrpc\":\"2.0\",\"method\":\"aeth_getSlotNumber\",\"params\":[],\"id\":1}'"
echo ""
echo "Stop with: ./scripts/devnet.sh stop"
echo "Clean with: ./scripts/devnet.sh clean"
