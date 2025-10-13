import { useMemo } from "react";

import { mockChainStats, mockJobs, mockValidators } from "../data/mock.js";

export function useExplorerData() {
  const stats = useMemo(() => mockChainStats, []);
  const validators = useMemo(() => mockValidators, []);
  const jobs = useMemo(() => mockJobs, []);
  return { stats, validators, jobs };
}
