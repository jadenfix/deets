import { JobBuilder, TransferBuilder } from "./builders.js";
import { Transaction } from "./transaction.js";
import {
  ClientConfig,
  DEFAULT_CONFIG,
  JobRequest,
  JobSubmission,
  SubmitResponse,
} from "./types.js";

function normalizeEndpoint(endpoint: string): string {
  return endpoint.replace(/\/+$/, "");
}

export class AetherClient {
  private readonly endpoint: string;
  constructor(
    endpoint: string,
    private readonly config: ClientConfig = DEFAULT_CONFIG,
  ) {
    if (!endpoint) {
      throw new Error("endpoint must be provided");
    }
    this.endpoint = normalizeEndpoint(endpoint);
  }

  static withConfig(endpoint: string, config: ClientConfig): AetherClient {
    return new AetherClient(endpoint, config);
  }

  getEndpoint(): string {
    return this.endpoint;
  }

  getConfig(): ClientConfig {
    return this.config;
  }

  transfer(): TransferBuilder {
    return new TransferBuilder(this.config);
  }

  job(): JobBuilder {
    return new JobBuilder(this.endpoint);
  }

  submit(transaction: Transaction): SubmitResponse {
    const txHash = transaction.hash();
    return {
      txHash,
      accepted: true,
    };
  }

  prepareJobSubmission(job: JobRequest): JobSubmission {
    return {
      url: `${this.endpoint}/v1/jobs`,
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: job,
    };
  }
}
