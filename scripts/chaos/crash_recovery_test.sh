#!/usr/bin/env bash
# Chaos test: node crash recovery
# Kills a validator, waits for it to restart, and verifies it catches up.
set -euo pipefail

DOWNTIME="${1:-30}"

echo "=== Aether Chaos: Node Crash Recovery ==="

VICTIM=$(kubectl get pods -l app=aether-validator -o name | shuf -n1)
VICTIM_NAME=$(echo "$VICTIM" | sed 's|pod/||')

echo "Victim: $VICTIM_NAME"
echo ""

# Record pre-crash slot
VICTIM_IP=$(kubectl get pod "$VICTIM_NAME" -o jsonpath='{.status.podIP}')
PRE_SLOT=$(curl -s "http://${VICTIM_IP}:8545" -X POST \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}' \
    | jq -r '.result // 0')
echo "Pre-crash slot: $PRE_SLOT"

echo ""
echo "--- Killing pod $VICTIM_NAME ---"
kubectl delete pod "$VICTIM_NAME" --grace-period=0 --force 2>/dev/null || true

echo "--- Waiting ${DOWNTIME}s (simulating downtime) ---"
sleep "$DOWNTIME"

echo ""
echo "--- Waiting for pod restart ---"
kubectl wait --for=condition=Ready "pod/$VICTIM_NAME" --timeout=120s

VICTIM_IP=$(kubectl get pod "$VICTIM_NAME" -o jsonpath='{.status.podIP}')

echo ""
echo "--- Waiting 15s for catch-up ---"
sleep 15

POST_SLOT=$(curl -s "http://${VICTIM_IP}:8545" -X POST \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}' \
    | jq -r '.result // 0')
echo "Post-recovery slot: $POST_SLOT"

# Get a healthy peer's slot for comparison
HEALTHY=$(kubectl get pods -l app=aether-validator -o name | grep -v "$VICTIM_NAME" | head -1)
HEALTHY_NAME=$(echo "$HEALTHY" | sed 's|pod/||')
HEALTHY_IP=$(kubectl get pod "$HEALTHY_NAME" -o jsonpath='{.status.podIP}')
HEALTHY_SLOT=$(curl -s "http://${HEALTHY_IP}:8545" -X POST \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}' \
    | jq -r '.result // 0')
echo "Healthy peer slot: $HEALTHY_SLOT"

DRIFT=$((HEALTHY_SLOT - POST_SLOT))
echo "Slot drift: $DRIFT"

if [ "$POST_SLOT" -gt "$PRE_SLOT" ] && [ "$DRIFT" -lt 5 ]; then
    echo ""
    echo "PASS: Node recovered and caught up (drift=$DRIFT slots)"
    exit 0
else
    echo ""
    echo "FAIL: Node did not recover properly (drift=$DRIFT slots)"
    exit 1
fi
