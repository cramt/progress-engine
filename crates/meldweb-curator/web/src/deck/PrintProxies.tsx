import { useState } from "react";
import type { Card } from "../deck";
import type { GitHubApi, RepoRef } from "../github/api";
import { loadCollection } from "../github/collection";
import {
  DEFAULT_LAYOUT,
  perPage,
  proxiesOf,
  proxySheet,
  type SheetLayout,
  type Sides,
  toPrint,
} from "../proxies";
import type { Printings } from "../scryfall";
import { downloadPdf } from "../ui/download";
import { PrintIcon } from "../ui/icons";
import { messageOf, Sheet } from "../ui/Sheet";
import { SheetLayoutFields } from "../ui/SheetLayoutFields";
import { printingNames } from "./archidektNames";

/**
 * Print proxies: a PDF of the deck's cards to print and cut, from Scryfall's
 * own images, as `metamorph deck` makes it (ADR-0038). The whole deck, or
 * what the collection lacks as the Sourcing view counts it.
 */
export function PrintProxies(props: {
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  name: string;
  text: string;
  cards: readonly Card[];
  printings: Printings;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        title="A PDF of the deck's cards to print and cut out"
      >
        <PrintIcon />
        Proxies…
      </button>
      {open && <ProxyDialog {...props} onClose={() => setOpen(false)} />}
    </>
  );
}

type Progress =
  | { kind: "idle" }
  | { kind: "working"; step: string }
  | { kind: "done"; summary: string };

function ProxyDialog({
  api,
  repo,
  path,
  name,
  text,
  cards,
  printings,
  onClose,
}: {
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  name: string;
  text: string;
  cards: readonly Card[];
  printings: Printings;
  onClose: () => void;
}) {
  const [lacking, setLacking] = useState(false);
  const [noBasics, setNoBasics] = useState(false);
  const [sides, setSides] = useState<Sides>("both");
  const [layout, setLayout] = useState<SheetLayout>(DEFAULT_LAYOUT);
  const [progress, setProgress] = useState<Progress>({ kind: "idle" });
  const [refusal, setRefusal] = useState<string | null>(null);
  const fits = (() => {
    try {
      return perPage(layout);
    } catch (e) {
      return messageOf(e);
    }
  })();
  const working = progress.kind === "working";

  const make = async () => {
    setRefusal(null);
    try {
      setProgress({ kind: "working", step: "Counting…" });
      const collection = lacking
        ? (await loadCollection(api, repo)).text
        : null;
      const counted = toPrint(
        path,
        text,
        collection,
        printingNames(cards, printings),
        noBasics,
      );
      if (counted.kind === "refused") throw new Error(counted.message);
      const found = proxiesOf(counted.lines, cards, printings, sides);
      if (found.kind === "unknown")
        throw new Error(
          `Scryfall knows no card for: ${found.names.join(", ")}`,
        );
      if (found.proxies.length === 0) throw new Error("Nothing left to print.");
      const pdf = await proxySheet(layout, found.proxies, (done, of) =>
        setProgress({
          kind: "working",
          step: `Fetching images ${done}/${of}…`,
        }),
      );
      downloadPdf(pdf, `${name}-proxies.pdf`);
      const sheets = found.proxies.reduce(
        (n, p) => n + p.qty * p.images.length,
        0,
      );
      const left = [
        lacking && `${counted.owned} owned`,
        noBasics && `${counted.basics} basic lands`,
      ].filter(Boolean);
      setProgress({
        kind: "done",
        summary: `${sheets} proxies on ${Math.ceil(sheets / (fits as number))} pages${
          left.length ? `, leaving out ${left.join(" and ")}` : ""
        }.`,
      });
    } catch (e) {
      setProgress({ kind: "idle" });
      setRefusal(messageOf(e));
    }
  };

  return (
    <Sheet label="Print proxies" onClose={onClose}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void make();
        }}
      >
        <h2>Print proxies</h2>
        <p className="hint">
          A PDF of Scryfall's own images at card size, with cut marks, made in
          this browser.
        </p>
        <fieldset className="choice">
          <legend className="visually-hidden">Which cards</legend>
          <label>
            <input
              type="radio"
              name="which"
              checked={!lacking}
              onChange={() => setLacking(false)}
            />
            The whole deck
          </label>
          <label>
            <input
              type="radio"
              name="which"
              checked={lacking}
              onChange={() => setLacking(true)}
            />
            What the collection lacks
          </label>
        </fieldset>
        <label className="check">
          <input
            type="checkbox"
            checked={noBasics}
            onChange={(e) => setNoBasics(e.target.checked)}
          />
          Leave out basic lands
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={sides === "front"}
            onChange={(e) => setSides(e.target.checked ? "front" : "both")}
          />
          Front side only for a card with two
        </label>
        <SheetLayoutFields layout={layout} onChange={setLayout} />
        {progress.kind === "done" && <p className="hint">{progress.summary}</p>}
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Close
          </button>
          <button
            type="submit"
            className="primary"
            disabled={working || typeof fits !== "number"}
          >
            {working ? progress.step : "Make PDF"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}
