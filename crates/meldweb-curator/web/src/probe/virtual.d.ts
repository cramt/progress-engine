// What vite.config.ts's gitaxianProbe plugin resolves `virtual:gitaxian-probe`
// to: Gitaxian Probe's JavaScript API (crates/gitaxian-probe/bindgen) in
// `MELDWEB_PROBE=1 pnpm dev`, and `null` in every other build and in tests.
declare module "virtual:gitaxian-probe" {
  export interface ScannerHandle {
    /** Delver X's build string. */
    readonly version: string;
    /** JSON: `Found[]`, see ../probe/scanner.ts. */
    scan(rgba: Uint8Array, width: number, height: number): Promise<string>;
    close(): void;
  }
  const probe: {
    init(): Promise<unknown>;
    Scanner: {
      open(
        base: string,
        onProgress?: (stage: string, percent: number, message: string) => void,
      ): Promise<ScannerHandle>;
    };
    /** Where the dev server serves Delver X's engine files. */
    base: string;
  } | null;
  export default probe;
}
