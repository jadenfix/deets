# Aether Incident Runbooks

## 1. Incident Triage

### Symptoms
- Alert fires from Prometheus (Slack / PagerDuty)
- User reports of stuck transactions or delayed finality

### Steps
1. Check Grafana dashboard: `Aether Overview > Finality Latency` panel.
2. Identify affected component from alert labels (`consensus`, `da`, `networking`, `runtime`).
3. SSH into affected validator or check pod logs:
   ```bash
   kubectl logs -l app=aether-validator --tail=200
   ```
4. Check peer connectivity:
   ```bash
   curl http://<validator>:8545 -X POST -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}'
   ```
5. Classify severity:
   - **P1**: Consensus halted, no finality for > 2 minutes
   - **P2**: Degraded throughput (< 1k TPS) or high latency (> 5s finality)
   - **P3**: Single node issue, network healthy

### Escalation
- P1: All hands, notify validators via broadcast channel
- P2: On-call engineer, 30-minute response
- P3: Next business day

---

## 2. Rollback / Roll-Forward

### When to Rollback
- Bad state committed due to consensus bug
- Validator running corrupted binary

### Rollback Steps
1. Stop affected validators:
   ```bash
   kubectl scale statefulset aether-validator --replicas=0
   ```
2. Identify last known good snapshot:
   ```bash
   ls -la /data/aether/snapshots/
   ```
3. Restore from snapshot:
   ```bash
   cp /data/aether/snapshots/<epoch>/state.db /data/aether/state.db
   ```
4. Restart with correct binary version:
   ```bash
   kubectl set image statefulset/aether-validator validator=aether/validator:<good-tag>
   kubectl scale statefulset aether-validator --replicas=4
   ```

### Roll-Forward
- If fix is available, deploy new version directly without snapshot restore
- Use `kubectl rollout restart statefulset/aether-validator`

---

## 3. Key Loss Response

### Validator Key Compromise
1. Trigger emergency unbond/slash through governance operations tooling.
2. Generate new Ed25519 identity key:
   ```bash
   aetherctl keys generate --out new-validator.key
   aetherctl keys show --path new-validator.key
   ```
3. Re-register validator with the new identity through node operator workflow.
4. Investigate compromise vector and document in post-mortem.

### KES Expiry
- KES keys auto-evolve each epoch (90-day lifecycle)
- If KES expires without rotation, validator cannot sign
- KES rotation commands are not yet exposed in `aetherctl`; use validator ops tooling.

---

## 4. Equivocation Response

### Detection
- Slashing proof submitted on-chain (double-sign detected)
- Alert: `AetherEquivocationDetected`

### Steps
1. Confirm equivocation evidence from chain telemetry/indexer output.
2. Identify root cause:
   - **Duplicate validator process**: Kill duplicate, check process management
   - **Network partition**: Validator saw two chain tips, both signed
   - **Malicious**: Ban validator permanently
3. Slashing is automatic (5% of stake for double-sign).
4. If accidental: validator can re-register after unbond period.
5. Document in incident report.

---

## 5. Degraded Network Handling

### Low Peer Count (< 3 peers)
1. Check node networking:
   ```bash
   curl http://<node>:8545 -X POST -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","method":"aeth_getSlotNumber","params":[],"id":1}'
   ```
2. Check firewall rules (ports 9000 TCP/UDP for P2P, 8545 for RPC).
3. If autodiscovery fails, reconfigure seed peers through node deployment config.

### High Packet Loss (> 20%)
1. Check DA metrics: `aether_da_packet_loss_ratio`
2. If localized to one region, check cloud provider status page.
3. If systemic, consider reducing block size or increasing RS parity shards.

### Finality Stalling
1. Check quorum: need 2/3 of stake voting.
2. Identify offline validators from `aether_consensus_votes_received` metric.
3. Contact offline validator operators.
4. If < 2/3 online, consensus halts by design (safety over liveness).
