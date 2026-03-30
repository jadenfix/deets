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
