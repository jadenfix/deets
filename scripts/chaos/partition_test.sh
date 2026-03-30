#!/usr/bin/env bash
# Chaos test: network partition simulation
# Simulates a 60/40 validator split for a configurable duration
# then heals and verifies no conflicting commits occurred.
set -euo pipefail

DURATION="${1:-60}"
PARTITION_RATIO="${2:-0.6}"

echo "=== Aether Chaos: Network Partition ==="
echo "Duration: ${DURATION}s, Partition ratio: ${PARTITION_RATIO}"

VALIDATORS=$(kubectl get pods -l app=aether-validator -o name)
TOTAL=$(echo "$VALIDATORS" | wc -l | tr -d ' ')
SPLIT=$(echo "$TOTAL * $PARTITION_RATIO" | bc | cut -d. -f1)

echo "Total validators: $TOTAL, Group A: $SPLIT, Group B: $((TOTAL - SPLIT))"

GROUP_A=$(echo "$VALIDATORS" | head -n "$SPLIT")
GROUP_B=$(echo "$VALIDATORS" | tail -n "+$((SPLIT + 1))")

echo ""
echo "--- Injecting partition ---"
for pod in $GROUP_B; do
    pod_name=$(echo "$pod" | sed 's|pod/||')
    echo "Blocking traffic to $pod_name from Group A"
    for a_pod in $GROUP_A; do
        a_name=$(echo "$a_pod" | sed 's|pod/||')
        a_ip=$(kubectl get pod "$a_name" -o jsonpath='{.status.podIP}')
        kubectl exec "$pod_name" -- iptables -A INPUT -s "$a_ip" -j DROP 2>/dev/null || true
        kubectl exec "$pod_name" -- iptables -A OUTPUT -d "$a_ip" -j DROP 2>/dev/null || true
    done
done

echo ""
echo "--- Partition active, waiting ${DURATION}s ---"
sleep "$DURATION"

echo ""
echo "--- Healing partition ---"
for pod in $GROUP_B; do
    pod_name=$(echo "$pod" | sed 's|pod/||')
    kubectl exec "$pod_name" -- iptables -F 2>/dev/null || true
done

echo ""
echo "--- Waiting 30s for re-convergence ---"
sleep 30

echo ""
echo "--- Checking for conflicting commits ---"
ROOTS=()
for pod in $VALIDATORS; do
    pod_name=$(echo "$pod" | sed 's|pod/||')
    pod_ip=$(kubectl get pod "$pod_name" -o jsonpath='{.status.podIP}')
    root=$(curl -s "http://${pod_ip}:8545" -X POST \
        -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"aeth_getStateRoot","params":[],"id":1}' \
        | jq -r '.result // "error"')
    echo "  $pod_name state_root=$root"
    ROOTS+=("$root")
done

UNIQUE=$(printf '%s\n' "${ROOTS[@]}" | sort -u | wc -l | tr -d ' ')
if [ "$UNIQUE" -eq 1 ]; then
    echo ""
    echo "PASS: All validators converged to same state root after partition heal"
    exit 0
else
    echo ""
    echo "FAIL: Validators have $UNIQUE distinct state roots after partition heal"
    exit 1
fi
