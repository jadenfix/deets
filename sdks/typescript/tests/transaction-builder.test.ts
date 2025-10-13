import assert from "node:assert/strict";
import { test } from "node:test";

import { AetherClient } from "../src/index.js";

test("transfer builder constructs deterministic transaction hash", () => {
  const client = new AetherClient("https://rpc.aether.local");
  const tx = client
    .transfer()
    .to("0x8b0b54d2248a3a5617b6bd8a2fd4cc8ebc0f2e90")
    .amount(1_000_000n)
    .memo("phase7-sdk")
    .fee(2_500_000n)
    .gasLimit(750_000)
    .build({
      sender: "0x1111111111111111111111111111111111111111",
      senderPublicKey:
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      signature:
        "0x".padEnd(2 + 128, "b"),
      nonce: 42,
    });

  const response = client.submit(tx);
  assert.equal(response.accepted, true);
  assert.ok(response.txHash.startsWith("0x"));
  assert.equal(
    response.txHash,
    tx.hash(),
    "client returns deterministic hash for submission",
  );
});

test("transfer builder validates required fields", () => {
  const client = new AetherClient("https://rpc.aether.local");
  const builder = client.transfer();

  assert.throws(
    () =>
      builder.build({
        sender: "0x111",
        senderPublicKey: "0x222",
        signature: "0x".padEnd(2 + 128, "1"),
        nonce: 0,
      }),
    /recipient not set/,
  );
});
