import suture from "./suture.svg?raw";

/**
 * What a route shows while its loader waits on GitHub or Scryfall. It looks
 * like index.html's splash, which it replaces on the first load; both draw
 * suture.svg, and index.html holds the styles.
 */
export function Loading() {
  return (
    <div className="boot" role="status">
      <span
        className="boot-spinner"
        // biome-ignore lint/security/noDangerouslySetInnerHtml: our own SVG, the one index.html inlines at build time
        dangerouslySetInnerHTML={{ __html: suture }}
      />
      <h1>Progress Engine</h1>
      <span className="boot-seam" />
      <p>Loading…</p>
      <span className="boot-credit">Font of Progress · Aaron J. Riley</span>
    </div>
  );
}
