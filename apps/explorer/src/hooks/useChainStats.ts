import { useEffect, useState } from "react";

import { AetherClient } from "@aether/sdk";
import { mockChainStats, mockJobs, mockValidators } from "../data/mock.js";
import type { ChainStats, JobInfo, ValidatorInfo } from "../types.js";

const DEFAULT_RPC_ENDPOINT = "http://127.0.0.1:8545";

export function useExplorerData() {
  const [stats, setStats] = useState<ChainStats>(mockChainStats);
  const [validators, setValidators] = useState<ValidatorInfo[]>(mockValidators);
  const [jobs, setJobs] = useState<JobInfo[]>(mockJobs);
  const [source, setSource] = useState<"live" | "mock">("mock");

  useEffect(() => {
    const endpoint =
      (globalThis as { __AETHER_RPC_ENDPOINT__?: string }).__AETHER_RPC_ENDPOINT__ ??
      DEFAULT_RPC_ENDPOINT;
    const client = new AetherClient(endpoint);
    let cancelled = false;

    const refresh = async () => {
      try {
        const [slot, finalized, block] = await Promise.all([
          client.getSlotNumber(),
          client.getFinalizedSlot(),
          client.getBlockByNumber("latest", true),
        ]);
        if (cancelled) {
          return;
        }

        const txCount = block?.transactions.length ?? 0;
        const proposer = formatAddress(block?.header?.proposer);
        setStats({
          slotTimeMs: 500,
          finalityLagSlots: Math.max(0, slot - finalized),
          tps: txCount * 2,
          uptimePercentage: 99.9,
        });
        setValidators(
          proposer
            ? [
                {
                  name: "Latest Proposer",
                  identity: proposer,
                  performanceScore: 99.0,
                  stake: 1_000_000,
                },
              ]
            : mockValidators,
        );
        setJobs(
          txCount === 0
            ? []
            : Array.from({ length: Math.min(3, txCount) }).map((_, index) => ({
                id: `slot-${slot}-tx-${index + 1}`,
                model: "inference",
                status: "running",
                provider: proposer || "unknown",
                maxFee: 500_000,
              })),
        );
        setSource("live");
      } catch {
        if (cancelled) {
          return;
        }
        setStats(mockChainStats);
        setValidators(mockValidators);
        setJobs(mockJobs);
        setSource("mock");
      }
    };

    refresh();
    const timer = setInterval(refresh, 5_000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  return { stats, validators, jobs, source };
}

function formatAddress(value: unknown): string | null {
  if (!value) {
    return null;
  }
  if (typeof value === "string") {
    return value;
  }
  if (Array.isArray(value) && value.every((item) => typeof item === "number")) {
    return "0x" + value.map((item) => item.toString(16).padStart(2, "0")).join("");
  }
  return null;
}
