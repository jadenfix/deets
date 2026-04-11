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
    assert.equal(tx.signature, "0x".padEnd(2 + 128, "b"));
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

test("transfer builder rejects short signatures", () => {
  const client = new AetherClient("https://rpc.aether.local");

  assert.throws(
    () =>
      client
        .transfer()
        .to("0x8b0b54d2248a3a5617b6bd8a2fd4cc8ebc0f2e90")
        .amount(1_000_000n)
        .build({
          sender: "0x1111111111111111111111111111111111111111",
          senderPublicKey:
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          signature: "0x" + "bb".repeat(32),
          nonce: 0,
        }),
    /signature must be exactly 64 bytes/,
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

test("getBlockByHash calls aeth_getBlockByHash", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  const fakeHash = "0x" + "ab".repeat(32);
  const fakeBlock = { header: { slot: 7, timestamp: 1000, proposer: null }, transactions: [] };
  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      assert.equal(payload.method, "aeth_getBlockByHash");
      assert.equal(payload.params[0], fakeHash);
      assert.equal(payload.params[1], true);
      return new Response(
        JSON.stringify({ jsonrpc: "2.0", id: payload.id, result: fakeBlock }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    const block = await client.getBlockByHash(fakeHash);
    assert.deepEqual(block, fakeBlock);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getBlockByHash passes fullTx=false when specified", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  const fakeHash = "0x" + "ab".repeat(32);
  const fakeBlock = { header: { slot: 7, timestamp: 1000, proposer: null }, transactions: [] };
  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      assert.equal(payload.params[0], fakeHash);
      assert.equal(payload.params[1], false);
      return new Response(
        JSON.stringify({ jsonrpc: "2.0", id: payload.id, result: fakeBlock }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    const block = await client.getBlockByHash(fakeHash, false);
    assert.deepEqual(block, fakeBlock);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getBlockByHash returns null for unknown hash", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      return new Response(
        JSON.stringify({ jsonrpc: "2.0", id: payload.id, result: null }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    const block = await client.getBlockByHash("0x" + "00".repeat(32));
    assert.equal(block, null);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getStateRoot calls aeth_getStateRoot without blockRef", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  const fakeRoot = "0x" + "cc".repeat(32);
  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      assert.equal(payload.method, "aeth_getStateRoot");
      assert.deepEqual(payload.params, []);
      return new Response(
        JSON.stringify({ jsonrpc: "2.0", id: payload.id, result: fakeRoot }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    const root = await client.getStateRoot();
    assert.equal(root, fakeRoot);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getStateRoot passes blockRef when provided", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  try {
    globalThis.fetch = async (_input, init) => {
      const payload = JSON.parse(init?.body?.toString() ?? "{}");
      assert.equal(payload.method, "aeth_getStateRoot");
      assert.deepEqual(payload.params, ["42"]);
      return new Response(
        JSON.stringify({ jsonrpc: "2.0", id: payload.id, result: "0x" + "dd".repeat(32) }),
        { status: 200, headers: { "content-type": "application/json" } },
      );
    };
    const root = await client.getStateRoot("42");
    assert.equal(root, "0x" + "dd".repeat(32));
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getHealth fetches /health endpoint", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  const fakeHealth = {
    status: "ok",
    version: "0.1.0",
    latestSlot: 100,
    finalizedSlot: 95,
    peerCount: 3,
    sync: { syncing: false },
  };
  try {
    globalThis.fetch = async (input, _init) => {
      assert.ok(
        String(input).endsWith("/health"),
        `expected /health URL, got: ${input}`,
      );
      return new Response(JSON.stringify(fakeHealth), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    };
    const health = await client.getHealth();
    assert.equal(health.status, "ok");
    assert.equal(health.latestSlot, 100);
    assert.equal(health.finalizedSlot, 95);
    assert.equal(health.peerCount, 3);
    assert.equal(health.sync.syncing, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("getHealth throws on non-200 status", async () => {
  const client = new AetherClient("http://rpc.aether.local");
  try {
    globalThis.fetch = async () =>
      new Response("Service Unavailable", { status: 503 });
    await assert.rejects(
      () => client.getHealth(),
      /health check failed with status 503/,
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
