export const probeOut: { glue: string; assets: string };
export const probePin: {
  version: string;
  tag: string;
  files: { name: string; sha256: string }[];
};
export function buildProbe(options?: { assets?: boolean }): void;
