declare module "node:assert/strict" {
  const assert: {
    equal(actual: unknown, expected: unknown, message?: string): void;
    deepEqual(actual: unknown, expected: unknown, message?: string): void;
    ok(value: unknown, message?: string): void;
    throws(fn: () => unknown, expected?: RegExp): void;
    rejects(fn: () => Promise<unknown>, expected?: RegExp): Promise<void>;
  };
  export default assert;
}

declare module "node:test" {
  export function test(
    name: string,
    fn: () => void | Promise<void>,
  ): void;
}

declare module "node:crypto" {
  export function createHash(algorithm: string): {
    update(data: string | Uint8Array): {
      update(data: string | Uint8Array): unknown;
      digest(encoding: "hex"): string;
    };
    digest(encoding: "hex"): string;
  };
}
