#!/bin/sh

set -eu

rpc() {
  host="$1"
  method="$2"
  params="$3"

  curl -fsS "http://${host}:8545" \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"${method}\",\"params\":${params},\"id\":1}"
}

expect_rpc_result() {
  host="$1"
  method="$2"
  params="$3"
  response="$(rpc "$host" "$method" "$params")"

  echo "$response" | grep -q "\"error\"" && {
    echo "RPC error from $host for $method: $response" >&2
    exit 1
  }

  echo "$response" | grep -q "\"result\"" || {
    echo "RPC result missing from $host for $method: $response" >&2
    exit 1
  }

  printf "%s" "$response"
}

for host in validator-1 validator-2 validator-3 validator-4; do
  curl -fsS "http://${host}:8545/health" >/dev/null
  expect_rpc_result "$host" "aeth_getSlotNumber" "[]" >/dev/null
done

for host in validator-1 validator-2 validator-3 validator-4; do
  attempt=1
  while [ "$attempt" -le 30 ]; do
    response="$(expect_rpc_result "$host" "aeth_getBlockByNumber" "[\"latest\",false]")"
    if ! echo "$response" | grep -q "\"result\":null"; then
      break
    fi

    if [ "$attempt" -eq 30 ]; then
      echo "Timed out waiting for a block on $host" >&2
      exit 1
    fi

    attempt=$((attempt + 1))
    sleep 1
  done
done

cargo test --all-features --workspace
