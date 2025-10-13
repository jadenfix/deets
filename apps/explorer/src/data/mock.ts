export interface ChainStats {
  slotTimeMs: number;
  finalityLagSlots: number;
  tps: number;
  uptimePercentage: number;
}

export interface ValidatorInfo {
  name: string;
  identity: string;
  performanceScore: number;
  stake: number;
}

export interface JobInfo {
  id: string;
  model: string;
  status: "pending" | "running" | "settled";
  provider: string;
  maxFee: number;
}

export const mockChainStats: ChainStats = {
  slotTimeMs: 520,
  finalityLagSlots: 2,
  tps: 12450,
  uptimePercentage: 99.92
};

export const mockValidators: ValidatorInfo[] = [
  { name: "Atlas One", identity: "atlas1", performanceScore: 98.2, stake: 1_250_000 },
  { name: "Singularity Labs", identity: "singularity", performanceScore: 97.6, stake: 980_000 },
  { name: "Nova Mesh", identity: "nova", performanceScore: 95.3, stake: 875_000 }
];

export const mockJobs: JobInfo[] = [
  {
    id: "job-1",
    model: "gpt-4-mini",
    status: "running",
    provider: "Nova Mesh",
    maxFee: 420_000
  },
  {
    id: "job-2",
    model: "diffusion-xl",
    status: "pending",
    provider: "Singularity Labs",
    maxFee: 310_000
  }
];
