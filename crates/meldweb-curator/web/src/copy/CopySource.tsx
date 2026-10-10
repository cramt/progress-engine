import { type CopyState, useCopyState } from "./index";

function source(state: CopyState): string {
  switch (state.kind) {
    case "ready":
      return `This browser keeps its own copy of Scryfall's card data, from ${new Date(state.updatedAt).toLocaleString()}, and checks for a newer one once a day. Prices are that day's. Pictures still come from Scryfall.`;
    case "opening":
      return "Reading this browser's copy of Scryfall's card data.";
    case "downloading":
    case "saving":
      return "Making this browser's copy of Scryfall's card data. Until it is done, cards come from Scryfall's API.";
    case "failed":
      return `Cards come from Scryfall's API: making a copy of its card data failed (${state.message}).`;
    case "unavailable":
      return `Cards come from Scryfall's API: this browser cannot keep a copy of its card data (${state.reason}).`;
  }
}

/** Where the editor's card facts come from (ADR-0030). */
export function CopySource() {
  const state = useCopyState();
  return (
    <section className="settings-rules" aria-labelledby="card-data">
      <h2 id="card-data">Card data</h2>
      <p className="settings-lede">{source(state)}</p>
    </section>
  );
}
