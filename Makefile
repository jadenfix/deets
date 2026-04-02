# ============================================================================
# AETHER - Build & Operations Makefile
# ============================================================================
# PURPOSE: Unified build, test, deploy interface for Aether blockchain
#
# WORKFLOW:
# Development: make build → make test → make devnet
# Testing: make testnet-deploy → make loadtest
# Production: make mainnet-build → make deploy-validator
# ============================================================================

.PHONY: all build test test-ts test-python test-all clean devnet testnet docs chaos validator-deploy \
        bench bench-parallel bench-consensus bench-mempool bench-ledger bench-storage bench-merkle \
        bench-runtime bench-crypto bench-da bench-rpc bench-types deny audit

# Default target
all: build test

# Build all crates
build:
	cargo build --release

# Development build (faster, debug symbols)
dev:
	cargo build

# Run all tests
test:
	./cli-test --rust-only

# Run TypeScript SDK tests
test-ts:
	cd sdks/typescript && npm ci && npm test

# Run Python SDK tests
test-python:
	cd sdks/python && pip install -e '.[dev]' && PYTHONPATH=src python -m pytest tests/ -v

# Run all tests (Rust + TypeScript + Python)
test-all: test test-ts test-python

# Run property tests
proptest:
	cargo test --all --features proptest -- --ignored

# Supply chain checks (cargo-deny)
deny:
	cargo deny check bans sources

# Security advisory audit
audit:
	cargo audit

# Lint and format
lint:
	./cli-format

# Format code
fmt:
	cargo fmt --all

# Generate documentation
docs:
	cargo doc --no-deps --all

# Clean build artifacts
clean:
	cargo clean
	docker compose -f deploy/docker/docker-compose.yml down -v

# ============================================================================
# Local Development
# ============================================================================

# Generate validator keys
keys:
	mkdir -p keys/
	cargo run -p aether-keytool -- generate --out keys/validator1.json
	cargo run -p aether-keytool -- generate --out keys/validator2.json
	cargo run -p aether-keytool -- generate --out keys/validator3.json
	cargo run -p aether-keytool -- generate --out keys/validator4.json

# Start local 4-node devnet
devnet: build
	docker compose -f deploy/docker/docker-compose.yml up --build -d

# Stop devnet
devnet-stop:
	docker compose -f deploy/docker/docker-compose.yml down

# View devnet logs
devnet-logs:
	docker compose -f deploy/docker/docker-compose.yml logs -f

# Initialize genesis
genesis:
	cargo run -p aether-node -- init-genesis \
		--config config/genesis.toml \
		--out genesis.json

# Run faucet (mint test tokens)
faucet:
	cargo run -p aether-faucet -- --amount 1000000 --to $(ADDR)

# ============================================================================
# Testing & Benchmarking
# ============================================================================

# Run load generator
loadtest:
	cargo run -p aether-loadgen --release -- \
		--rpc http://localhost:8545 \
		--tps 5000 \
		--duration 300

# Run chaos tests
chaos:
	./scripts/chaos/run-chaos-suite.sh

# Benchmark parallel execution (legacy alias)
bench-parallel:
	cargo bench --package aether-runtime --bench scheduler

# Individual benchmark targets
bench-consensus:
	cargo bench --package aether-consensus

bench-mempool:
	cargo bench --package aether-mempool

bench-ledger:
	cargo bench --package aether-ledger

bench-storage:
	cargo bench --package aether-state-storage

bench-merkle:
	cargo bench --package aether-state-merkle

bench-runtime:
	cargo bench --package aether-runtime

bench-crypto:
	cargo bench --package aether-crypto-primitives
	cargo bench --package aether-crypto-vrf

bench-da:
	cargo bench --package aether-da-erasure
	cargo bench --package aether-da-turbine

bench-rpc:
	cargo bench --package aether-rpc-json

bench-types:
	cargo bench --package aether-types

# Run all benchmark suites across the workspace
bench:
	@echo "==> Running all criterion benchmarks"
	cargo bench --package aether-consensus
	cargo bench --package aether-mempool
	cargo bench --package aether-ledger
	cargo bench --package aether-state-storage
	cargo bench --package aether-state-merkle
	cargo bench --package aether-runtime
	cargo bench --package aether-crypto-primitives
	cargo bench --package aether-crypto-vrf
	cargo bench --package aether-da-erasure
	cargo bench --package aether-da-turbine
	cargo bench --package aether-rpc-json
	cargo bench --package aether-types
	@echo "==> All benchmarks complete"

# ============================================================================
# Deployment (K8s)
# ============================================================================

# Deploy validator manifests
validator-deploy:
	kubectl apply -f deploy/k8s/validator/

# Deploy testnet validator
testnet-deploy: validator-deploy

# Deploy mainnet validator
mainnet-deploy: validator-deploy

# Deploy monitoring stack
monitoring:
	helm upgrade --install prometheus deploy/helm/monitoring/ \
		--namespace aether-monitoring --create-namespace

# ============================================================================
# AI Mesh
# ============================================================================

# Build deterministic AI worker image
ai-worker-build:
	cd ai-mesh/runtime && docker build -t aether-ai-worker:latest .

# Generate model hash
model-hash:
	@test -n "$(MODEL_PATH)" || (echo "MODEL_PATH is required" >&2; exit 1)
	shasum -a 256 "$(MODEL_PATH)" | awk '{print $$1}'

# ============================================================================
# Utilities
# ============================================================================

# Check chain status
status:
	cargo run -p aether-cli -- status --rpc http://localhost:8545

# Submit transaction
submit-tx:
	cargo run -p aether-cli -- submit --tx $(TX_FILE)

# Query block
query-block:
	cargo run -p aether-cli -- block --number $(BLOCK_NUM)
