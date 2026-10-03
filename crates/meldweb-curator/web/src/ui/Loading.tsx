/**
 * What a route shows while its loader waits on GitHub or Scryfall. It looks
 * like index.html's splash, which it replaces on the first load.
 */
export function Loading() {
  return (
    <div className="boot" role="status">
      <h1>Meldweb Curator</h1>
      <div className="boot-spinner" />
      <p>Loading…</p>
    </div>
  );
}
