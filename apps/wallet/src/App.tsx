import React, { useMemo } from "react";

import { AetherClient } from "@aether/sdk";
import { Card, Metric, Section } from "@aether/ui";

const client = new AetherClient("https://rpc.aether.local");

interface TransferPreview {
  txHash: string;
  sender: string;
  recipient: string;
  amount: string;
  fee: string;
}

interface JobPreview {
  url: string;
  method: string;
  jobId: string;
  modelHash: string;
  preparedMatches: boolean;
}

const DEMO_SENDER = "0x1111111111111111111111111111111111111111";
const DEMO_PUBLIC_KEY = "0x" + "aa".repeat(32);
const DEMO_SIGNATURE = "0x" + "bb".repeat(64);
const DEMO_RECIPIENT = "0x8b0b54d2248a3a5617b6bd8a2fd4cc8ebc0f2e90";

export function App() {
  const transferPreview = useMemo<TransferPreview>(() => {
    const tx = client
      .transfer()
      .to(DEMO_RECIPIENT)
      .amount(750_000n)
      .memo("wallet demo")
      .fee(2_750_000n)
      .gasLimit(650_000)
      .build({
        sender: DEMO_SENDER,
        senderPublicKey: DEMO_PUBLIC_KEY,
        signature: DEMO_SIGNATURE,
        nonce: 7
      });

    const response = client.submit(tx);
    return {
      txHash: response.txHash,
      sender: DEMO_SENDER,
      recipient: DEMO_RECIPIENT,
      amount: "750,000 AIC",
      fee: "2,750,000 lamports"
    };
  }, []);

  const jobPreview = useMemo<JobPreview>(() => {
    const submission = client
      .job()
      .id("wallet-hello-job")
      .model("0x" + "12".repeat(32))
      .input("0x" + "ab".repeat(32))
      .maxFee(600_000_000)
      .expiresAt(new Date(Date.now() + 3_600_000))
      .toSubmission();

    const prepared = client.prepareJobSubmission(submission.body);
    return {
      url: prepared.url,
      method: prepared.method,
      jobId: prepared.body.jobId,
      modelHash: prepared.body.modelHash,
      preparedMatches: prepared.body.jobId === "wallet-hello-job"
    };
  }, []);

  return (
    <div style={{
      display: "grid",
      gap: "24px",
      padding: "24px",
      background: "#050b12",
      minHeight: "100vh",
      fontFamily: "Inter, system-ui, -apple-system, BlinkMacSystemFont"
    }}>
      <Card title="Wallet Overview" subtitle="Phase 7 SDK wiring">
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))", gap: "12px" }}>
          <Metric label="Default Endpoint" value={client.getEndpoint()} />
          <Metric label="Demo Sender" value={`${transferPreview.sender.slice(0, 10)}…`} />
          <Metric label="Transfer Hash" value={transferPreview.txHash.slice(0, 12) + "…"} />
        </div>
      </Card>

      <Card title="Transfer Preview" subtitle="Offline payload">
        <Section title="Details">
          <dl style={{ display: "grid", gap: "4px", gridTemplateColumns: "auto 1fr", color: "#e5ecff" }}>
            <dt>Recipient</dt>
            <dd style={{ margin: 0 }}>{transferPreview.recipient}</dd>
            <dt>Amount</dt>
            <dd style={{ margin: 0 }}>{transferPreview.amount}</dd>
            <dt>Fee</dt>
            <dd style={{ margin: 0 }}>{transferPreview.fee}</dd>
          </dl>
        </Section>
      </Card>

      <Card title="Job Submission" subtitle="Hello AIC tutorial">
        <Section title="Coordinator Request">
          <dl style={{ display: "grid", gap: "4px", gridTemplateColumns: "auto 1fr", color: "#e5ecff" }}>
            <dt>Endpoint</dt>
            <dd style={{ margin: 0 }}>{jobPreview.url}</dd>
            <dt>Method</dt>
            <dd style={{ margin: 0 }}>{jobPreview.method}</dd>
            <dt>Job ID</dt>
            <dd style={{ margin: 0 }}>{jobPreview.jobId}</dd>
            <dt>Model</dt>
            <dd style={{ margin: 0 }}>{jobPreview.modelHash.slice(0, 18) + "…"}</dd>
            <dt>Prepared Matches</dt>
            <dd style={{ margin: 0 }}>{jobPreview.preparedMatches ? "yes" : "no"}</dd>
          </dl>
        </Section>
      </Card>
    </div>
  );
}
