#!/usr/bin/env bash
# Chaos test: packet loss injection
# Uses tc (traffic control) to inject packet loss and verifies DA layer handles it.
set -euo pipefail

LOSS_PERCENT="${1:-10}"
DURATION="${2:-60}"

echo "=== Aether Chaos: Packet Loss ==="
echo "Loss: ${LOSS_PERCENT}%, Duration: ${DURATION}s"

VALIDATORS=$(kubectl get pods -l app=aether-validator -o name)
TOTAL=$(echo "$VALIDATORS" | wc -l | tr -d ' ')

echo "Injecting ${LOSS_PERCENT}% packet loss on all $TOTAL validators"

for pod in $VALIDATORS; do
    pod_name=$(echo "$pod" | sed 's|pod/||')
    kubectl exec "$pod_name" -- tc qdisc add dev eth0 root netem loss "${LOSS_PERCENT}%" 2>/dev/null || \
    kubectl exec "$pod_name" -- tc qdisc change dev eth0 root netem loss "${LOSS_PERCENT}%" 2>/dev/null || true
done

echo "--- Packet loss active, waiting ${DURATION}s ---"
sleep "$DURATION"

echo ""
echo "--- Removing packet loss ---"
for pod in $VALIDATORS; do
    pod_name=$(echo "$pod" | sed 's|pod/||')
    kubectl exec "$pod_name" -- tc qdisc del dev eth0 root 2>/dev/null || true
done

echo ""
echo "--- Checking finality ---"
sleep 10

FINALIZED_SLOTS=()
for pod in $VALIDATORS; do
    pod_name=$(echo "$pod" | sed 's|pod/||')
    pod_ip=$(kubectl get pod "$pod_name" -o jsonpath='{.status.podIP}')
    slot=$(curl -s "http://${pod_ip}:8545" -X POST \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"aeth_getFinalizedSlot","params":[],"id":1}' \
        | jq -r '.result // 0')
    echo "  $pod_name finalized_slot=$slot"
    FINALIZED_SLOTS+=("$slot")
done

MAX_SLOT=$(printf '%s\n' "${FINALIZED_SLOTS[@]}" | sort -rn | head -1)
MIN_SLOT=$(printf '%s\n' "${FINALIZED_SLOTS[@]}" | sort -n | head -1)
DRIFT=$((MAX_SLOT - MIN_SLOT))

if [ "$MIN_SLOT" -gt 0 ] && [ "$DRIFT" -lt 10 ]; then
    echo ""
    echo "PASS: Chain continued finalizing under ${LOSS_PERCENT}% packet loss (drift=$DRIFT)"
    exit 0
else
    echo ""
    echo "FAIL: Finality degraded under packet loss (min=$MIN_SLOT, max=$MAX_SLOT, drift=$DRIFT)"
    exit 1
fi
