# Phase 7 Incentives & Grants

The developer ecosystem ships with a standing incentive program to keep the testnet healthy. This page captures the workflows for builders, validators, and bug hunters now that the SDKs, explorer, and faucet are live.

## Faucet Service

The faucet exposes a JSON API at `POST /request` and is backed by the `aether-faucet` crate. Requests are rate-limited per GitHub handle and capped per token to protect testnet liquidity.

```bash
AETHER_FAUCET_ADDR=0.0.0.0:8080 cargo run -p aether-faucet --bin server
```

Request body:

```json
{
  "github": "meshbuilder",
  "address": "0x8b0b54d2248a3a5617b6bd8a2fd4cc8ebc0f2e90",
  "token": "AIC",
  "amount": 150000
}
```

All responses include an envelope describing the grant and a memo string that can be traced in telemetry dashboards.

| Field     | Description                             |
|-----------|-----------------------------------------|
| `status`  | `accepted` or `rejected`                |
| `message` | Human readable outcome                  |
| `grant`   | Present only for accepted requests      |

The default configuration allows up to 250k AIC or SWR per 10 minute window. Operators can customise limits via `FaucetConfig` or by exporting `AETHER_FAUCET_LIMIT` and `AETHER_FAUCET_COOLDOWN` in the systemd unit.

### Local Development

Use the helper script in `scripts/run_faucet_local.sh` to launch the service with trace logging enabled:

```bash
./scripts/run_faucet_local.sh
```

Test suites under `crates/tools/faucet` simulate rate limits, token allow-list checks, and JSON payload validation.

## Validator Scorecards

The `aether-scorecard` crate converts metrics snapshots (JSON) into Markdown and CSV artefacts that we publish weekly. Scores are derived from uptime, latency, finality faults, and missed slots.

```bash
cargo run -p aether-scorecard --bin scorecard \
  --input metrics/validators.json \
  --markdown-out out/scorecard.md \
  --csv-out out/scorecard.csv
```

Score interpretation:

| Grade | Score Range | Notes                              |
|-------|-------------|------------------------------------|
| A     | 90 – 100    | Eligible for grants + spotlight    |
| B     | 75 – <90    | Solid performance                  |
| C     | 60 – <75    | Needs investigation / support      |
| D     | <60         | Ineligible; remediation required   |

The generated Markdown table is designed for direct inclusion in community updates. CSV exports feed a data warehouse job that ranks validators by performance over the last epoch.

## Bug Bounties & Grants

1. **File a security disclosure** – email `security@aether.foundation` or open a ticket in the private bug bounty repository.
2. **Run the Phase 7 acceptance suite** – attach the output of `scripts/run_phase7_acceptance.sh` so we can reproduce the issue with the latest SDK builds.
3. **Rewards** – issues are triaged against the risk matrix; eligible reports receive AIC credits via the faucet automation.

Validators meeting the scorecard A-grade threshold and maintaining uptime above 99% across four consecutive epochs are auto-enrolled into the grants pilot. Additional grants are awarded to teams shipping end-to-end tutorials or reference integrations with the explorer and wallet codebases.
