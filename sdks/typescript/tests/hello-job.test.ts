import assert from "node:assert/strict";
import { test } from "node:test";

import { AetherClient } from "../src/index.js";

test("hello AIC job tutorial flow builds submission envelope", () => {
  const client = new AetherClient("https://rpc.aether.local");

  const submission = client
    .job()
    .id("hello-aic-job")
    .model("0x" + "12".repeat(32))
    .input("0x" + "ab".repeat(32))
    .maxFee(500_000_000n)
    .expiresAt(new Date(Date.now() + 60 * 60 * 1000))
    .withMetadata({
      prompt: "Generate a haiku about verifiable compute.",
      priority: "gold",
    })
    .toSubmission();

  assert.equal(submission.url, "https://rpc.aether.local/v1/jobs");
  assert.equal(submission.method, "POST");
  assert.equal(
    submission.headers["content-type"],
    "application/json",
    "submission sets JSON content type",
  );

  const prepared = client.prepareJobSubmission(submission.body);
  assert.deepEqual(prepared, submission);
  assert.equal(submission.body.jobId, "hello-aic-job");
  assert.equal(submission.body.maxFee, 500_000_000n);
  assert.ok(submission.body.expiresAt > Math.floor(Date.now() / 1000));
  assert.equal(
    submission.body.metadata?.prompt,
    "Generate a haiku about verifiable compute.",
  );
});
