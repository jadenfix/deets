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

.PHONY: all build test clean devnet testnet docs chaos validator-deploy

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

# Run property tests
proptest:
	cargo test --all --features proptest -- --ignored

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

# Benchmark parallel execution
bench-parallel:
	cargo bench --package aether-runtime --bench scheduler

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
