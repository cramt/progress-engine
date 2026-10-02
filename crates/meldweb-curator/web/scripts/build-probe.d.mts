export const probeOut: { glue: string; assets: string };
type Pinned = { name: string; sha256: string };
export const probePin: {
  version: string;
  engine: Pinned[];
  tiers: Record<"alpha" | "lambda" | "gamma", { tag: string; model: Pinned[] }>;
};
export function buildProbe(options?: { assets?: boolean }): void;
