#!/bin/sh
# Docker entrypoint for Aether validator nodes.
# Runs genesis ceremony if genesis.json doesn't exist yet, then starts the node.
set -e

GENESIS_DIR="${AETHER_GENESIS_DIR:-/data/genesis}"
GENESIS_PATH="${GENESIS_DIR}/genesis.json"
VALIDATOR_ID="${VALIDATOR_ID:-1}"
NUM_VALIDATORS="${NUM_VALIDATORS:-4}"

# If this is validator-1 and no genesis exists, run the ceremony
if [ "$VALIDATOR_ID" = "1" ] && [ ! -f "$GENESIS_PATH" ]; then
    echo "Running genesis ceremony for ${NUM_VALIDATORS} validators..."
    genesis-ceremony \
        --validators "$NUM_VALIDATORS" \
        --output-dir "$GENESIS_DIR" \
        --stake 1000000 \
        --network devnet
fi

# Wait for genesis.json to appear (other validators wait for validator-1)
ATTEMPTS=0
while [ ! -f "$GENESIS_PATH" ]; do
    ATTEMPTS=$((ATTEMPTS + 1))
    if [ "$ATTEMPTS" -gt 30 ]; then
        echo "ERROR: genesis.json not found after 30s at $GENESIS_PATH"
        exit 1
    fi
    echo "Waiting for genesis.json... (${ATTEMPTS}s)"
    sleep 1
done

# Set environment for the node
export AETHER_GENESIS_PATH="$GENESIS_PATH"
export AETHER_VALIDATOR_KEY="${GENESIS_DIR}/validator-${VALIDATOR_ID}.key"
export AETHER_NODE_DB_PATH="/data/validator-${VALIDATOR_ID}"

echo "Starting validator ${VALIDATOR_ID}..."
echo "  Genesis: $AETHER_GENESIS_PATH"
echo "  Key:     $AETHER_VALIDATOR_KEY"
echo "  DB:      $AETHER_NODE_DB_PATH"

exec aether-node "$@"
