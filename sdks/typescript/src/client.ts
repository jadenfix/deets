import { JobBuilder, TransferBuilder } from "./builders.js";
import { Transaction } from "./transaction.js";
import {
  ClientConfig,
  DEFAULT_CONFIG,
  JobRequest,
  JobSubmission,
  NodeHealth,
  RpcAccountState,
  RpcBlock,
  RpcReceipt,
  SubmitResponse,
} from "./types.js";

function normalizeEndpoint(endpoint: string): string {
  return endpoint.replace(/\/+$/, "");
}

export class AetherClient {
  private readonly endpoint: string;
  private requestId = 1;
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

  async submit(transaction: Transaction): Promise<SubmitResponse> {
    const txHash = await this.rpcCall<string>("aeth_sendTransaction", [
      transaction.toRpcTransaction(),
    ]);
    return {
      txHash,
      accepted: true,
    };
  }

  async getSlotNumber(): Promise<number> {
    return this.rpcCall<number>("aeth_getSlotNumber", []);
  }

  async getFinalizedSlot(): Promise<number> {
    return this.rpcCall<number>("aeth_getFinalizedSlot", []);
  }

  async getBlockByNumber(
    blockRef: number | "latest" = "latest",
    fullTx = true,
  ): Promise<RpcBlock | null> {
    return this.rpcCall<RpcBlock | null>("aeth_getBlockByNumber", [
      blockRef.toString(),
      fullTx,
    ]);
  }

  async getTransactionReceipt(txHash: string): Promise<RpcReceipt | null> {
    return this.rpcCall<RpcReceipt | null>("aeth_getTransactionReceipt", [txHash]);
  }

  async getAccount(
    address: string,
    blockRef?: string,
  ): Promise<RpcAccountState | null> {
    const params = blockRef ? [address, blockRef] : [address];
    return this.rpcCall<RpcAccountState | null>("aeth_getAccount", params);
  }

  async getBlockByHash(blockHash: string): Promise<RpcBlock | null> {
    return this.rpcCall<RpcBlock | null>("aeth_getBlockByHash", [blockHash, true]);
  }

  async getStateRoot(blockRef?: string): Promise<string> {
    const params = blockRef ? [blockRef] : [];
    return this.rpcCall<string>("aeth_getStateRoot", params);
  }

  /**
   * Fetch node health from the HTTP `/health` endpoint (not a JSON-RPC call).
   * Returns sync status, peer count, and latest/finalized slot numbers.
   */
  async getHealth(): Promise<NodeHealth> {
    const response = await fetch(`${this.endpoint}/health`);
    if (!response.ok) {
      throw new Error(`health check failed with status ${response.status}`);
    }
    return response.json() as Promise<NodeHealth>;
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

  private async rpcCall<T>(method: string, params: unknown[]): Promise<T> {
    const response = await fetch(this.endpoint, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        jsonrpc: "2.0",
        method,
        params,
        id: this.requestId++,
      }),
    });

    if (!response.ok) {
      throw new Error(`rpc request failed with status ${response.status}`);
    }

    const payload = (await response.json()) as {
      result?: T;
      error?: { code: number; message: string };
    };
    if (payload.error) {
      throw new Error(`rpc error ${payload.error.code}: ${payload.error.message}`);
    }
    if (!("result" in payload)) {
      throw new Error("rpc response missing result");
    }
    return payload.result as T;
  }
}
