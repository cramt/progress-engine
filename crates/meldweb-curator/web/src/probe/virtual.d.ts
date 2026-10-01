// What vite.config.ts's gitaxianProbe plugin resolves `virtual:gitaxian-probe`
// to: Gitaxian Probe's JavaScript API (crates/gitaxian-probe/bindgen) in every
// site build and in `MELDWEB_PROBE=1 pnpm dev`, and `null` in plain `pnpm dev`
// and in tests.
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
        files?: Record<string, string>,
      ): Promise<ScannerHandle>;
    };
    base: string;
    /** Each of Delver X's engine files, by name: `/gitaxian-probe/<sha256>/<name>`. */
    files: Record<string, string>;
  } | null;
  export default probe;
}
