# Getting Started

This guide takes a contributor from clone to a running node, a local multi-node environment, and the validation commands that mirror the current GitHub Actions workflow.

## Prerequisites

Required:

- Rust stable toolchain
- Git

Recommended:

- Docker and Docker Compose for the Compose-based test environment
- Node.js 20+ and `npm` if you plan to work on the TypeScript SDK or web applications

## 1. Clone and Build

```bash
git clone https://github.com/jadenfix/deets.git
cd deets

cargo build --workspace
```

## 2. Run a Single Node

```bash
cargo run -p aether-node
```

Default behavior:

- Uses the `devnet` chain preset unless `AETHER_CONFIG_PATH` or `AETHER_NETWORK` overrides it.
- Stores data under `./data/node1`.
- Generates a validator key at `./data/node1/validator.key` if one does not already exist.
- Starts JSON-RPC on `127.0.0.1:8545`.
- Starts the P2P listener on port `9000`.

Check that the node is alive:

```bash
curl -s http://127.0.0.1:8545/health
```

Query the current slot:

```bash
curl -s http://127.0.0.1:8545 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}'
```

Stop the node with `Ctrl-C`.

## 3. Run the CI-Aligned Validation Flow

Lint and static checks:

```bash
./scripts/lint.sh
```

Workspace tests:

```bash
./scripts/test.sh
```

Those scripts mirror the repository’s current Rust-focused CI behavior more closely than ad hoc commands.

## 4. Start a Local Multi-Node Devnet

The repository includes a process-based devnet launcher:

```bash
./scripts/devnet.sh
```

What it does:

- builds `aether-node` in release mode;
- starts four local validators;
- assigns RPC ports `8545` through `8548`;
- assigns P2P ports `9000` through `9003`; and
- writes logs and data under `./data/devnet/`.

Useful commands:

```bash
./scripts/devnet.sh stop
./scripts/devnet.sh clean
```

Inspect logs in `./data/devnet/node*.log`.

## 5. Run the Docker-Based Test Network

The integration environment used by CI is described in `docker-compose.test.yml`.

Quick path:

```bash
./scripts/docker-test.sh
```

Manual path:

```bash
docker compose -f docker-compose.test.yml build
docker compose -f docker-compose.test.yml up -d validator-1 validator-2 validator-3 validator-4
docker compose -f docker-compose.test.yml run test-runner
docker compose -f docker-compose.test.yml down
```

## 6. Explore the CLI and SDK Surfaces

CLI:

```bash
cargo run -p aether-cli --bin aetherctl -- --help
```

TypeScript and web workspaces:

```bash
npm install
npm run test:ts
```

Python SDK metadata lives under `sdks/python/`, and the TypeScript SDK lives under `sdks/typescript/`.

## 7. Useful Environment Variables

`aether-node` supports the following inputs:

| Variable | Purpose |
| --- | --- |
| `AETHER_NETWORK` | Selects a built-in chain preset such as `devnet`, `testnet`, or `mainnet`. |
| `AETHER_CONFIG_PATH` | Loads a chain configuration from a TOML file instead of a preset. |
| `AETHER_NODE_DB_PATH` | Overrides the node data directory. |
| `AETHER_VALIDATOR_KEY` | Overrides the validator key path. |
| `AETHER_GENESIS_PATH` | Loads multi-validator genesis JSON instead of single-validator quick-start mode. |
| `AETHER_RPC_PORT` | Overrides the JSON-RPC port. |
| `AETHER_P2P_PORT` | Overrides the P2P listener port. |
| `AETHER_BOOTSTRAP_PEERS` | Comma-separated peer addresses for outbound bootstrapping. |

## 8. Where to Go Next

- `overview.md` for the project and repository map.
- `docs/architecture.md` for the current system design.
- `docs/ops/RUNBOOKS.md` for operational workflows.
- `CONTRIBUTING.md` for PR and validation expectations.
