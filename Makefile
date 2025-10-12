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

.PHONY: all build test clean devnet testnet docs

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
	cargo test --all
	cargo test --all --release

# Run property tests
proptest:
	cargo test --all --features proptest -- --ignored

# Lint and format
lint:
	cargo fmt --all -- --check
	cargo clippy --all -- -D warnings

# Format code
fmt:
	cargo fmt --all

# Generate documentation
docs:
	cargo doc --no-deps --all

# Clean build artifacts
clean:
	cargo clean
	rm -rf target/
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
	cd tests/chaos && ./run-chaos-suite.sh

# Benchmark parallel execution
bench-parallel:
	cargo bench --package aether-runtime --bench scheduler

# ============================================================================
# Deployment (K8s)
# ============================================================================

# Deploy testnet validator
testnet-deploy:
	kubectl apply -f deploy/k8s/testnet/

# Deploy mainnet validator
mainnet-deploy:
	kubectl apply -f deploy/k8s/mainnet/

# Deploy monitoring stack
monitoring:
	helm install prometheus deploy/k8s/charts/monitoring/ \
		--namespace aether-monitoring --create-namespace

# ============================================================================
# AI Mesh
# ============================================================================

# Build deterministic AI worker image
ai-worker-build:
	cd ai-mesh/runtime && docker build -t aether-ai-worker:latest .

# Generate model hash
model-hash:
	cargo run -p aether-models -- hash --model $(MODEL_PATH)

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

