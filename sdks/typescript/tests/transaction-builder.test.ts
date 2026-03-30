import assert from "node:assert/strict";
import { test } from "node:test";

import { AetherClient } from "../src/index.js";

const originalFetch = globalThis.fetch;

test("transfer builder submits over JSON-RPC", async () => {
  const client = new AetherClient("http://rpc.aether.local");
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

  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      assert.equal(payload.method, "aeth_sendTransaction");
      assert.equal(payload.params[0].recipient, tx.recipient);
      return new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          id: payload.id,
          result:
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    };

    const response = await client.submit(tx);
    assert.equal(response.accepted, true);
    assert.equal(
      response.txHash,
      "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
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

test("slot query reads from RPC", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      assert.equal(payload.method, "aeth_getSlotNumber");
      return new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          id: payload.id,
          result: 123,
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    };
    const slot = await client.getSlotNumber();
    assert.equal(slot, 123);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
