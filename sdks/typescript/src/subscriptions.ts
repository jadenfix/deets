/**
 * WebSocket subscription client for Aether chain events.
 *
 * Connects to the node's `/ws` endpoint and receives real-time
 * notifications for new blocks, finality updates, and transactions.
 *
 * Usage:
 * ```ts
 * const sub = new AetherSubscription("ws://localhost:8545/ws");
 * sub.on("newBlock", (block) => console.log("New block:", block.slot));
 * sub.on("finality", (event) => console.log("Finalized:", event.finalizedSlot));
 * await sub.connect();
 * ```
 */

export interface BlockEvent {
  slot: number;
  hash: string;
  proposer: string;
  txCount: number;
  timestamp: number;
}

export interface FinalityEvent {
  finalizedSlot: number;
  blockHash: string;
}

export interface SubscriptionEvent {
  topic: string;
  data: BlockEvent | FinalityEvent | Record<string, unknown>;
}

type EventHandler<T> = (data: T) => void;

export class AetherSubscription {
  private ws: WebSocket | null = null;
  private handlers: Map<string, EventHandler<any>[]> = new Map();
  private reconnectAttempts = 0;
  private maxReconnectAttempts = 10;
  private reconnectDelayMs = 1000;

  constructor(
    private readonly wsUrl: string,
    private readonly autoReconnect = true,
  ) {}

  /**
   * Register a handler for a specific event topic.
   */
  on(topic: "newBlock", handler: EventHandler<BlockEvent>): this;
  on(topic: "finality", handler: EventHandler<FinalityEvent>): this;
  on(topic: string, handler: EventHandler<Record<string, unknown>>): this;
  on(topic: string, handler: EventHandler<any>): this {
    const existing = this.handlers.get(topic) || [];
    existing.push(handler);
    this.handlers.set(topic, existing);
    return this;
  }

  /**
   * Connect to the WebSocket endpoint.
   */
  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.wsUrl);

        this.ws.onopen = () => {
          this.reconnectAttempts = 0;
          resolve();
        };

        this.ws.onmessage = (event: MessageEvent) => {
          try {
            const parsed = JSON.parse(
              typeof event.data === "string" ? event.data : "",
            ) as SubscriptionEvent;
            this.dispatch(parsed);
          } catch {
            // Ignore malformed messages
          }
        };

        this.ws.onclose = () => {
          if (this.autoReconnect && this.reconnectAttempts < this.maxReconnectAttempts) {
            this.reconnectAttempts++;
            const delay = this.reconnectDelayMs * Math.pow(2, this.reconnectAttempts - 1);
            setTimeout(() => this.connect(), delay);
          }
        };

        this.ws.onerror = (err) => {
          if (this.reconnectAttempts === 0) {
            reject(new Error("WebSocket connection failed"));
          }
        };
      } catch (err) {
        reject(err);
      }
    });
  }

  /**
   * Disconnect from the WebSocket.
   */
  disconnect(): void {
    this.maxReconnectAttempts = 0; // Prevent reconnection
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
  }

  /**
   * Check if connected.
   */
  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  private dispatch(event: SubscriptionEvent): void {
    const handlers = this.handlers.get(event.topic);
    if (handlers) {
      for (const handler of handlers) {
        try {
          handler(event.data);
        } catch {
          // Don't let handler errors crash the subscription
        }
      }
    }
  }
}
