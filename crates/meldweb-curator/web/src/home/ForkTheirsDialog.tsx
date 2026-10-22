import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { slugify } from "../github/decks";
import {
  forkTheirs,
  freeStem,
  ownerOf,
  type TheirDecks,
  theirDecks,
  theirRepoUrl,
} from "../remote/follow";
import type { Repo } from "../repo/api";
import { Sheet, useBusy } from "../ui/Sheet";
import { Waiting } from "../ui/Spinner";

type Looked = { url: string; decks: TheirDecks };

/**
 * Forks a deck from another player's public repo (ADR-0041): their GitHub
 * login or any git URL, then one of their decks, then the stem it is kept
 * under here. Only that deck's line comes down, with their commits and
 * authors, and the new deck follows theirs, to take their changes later.
 */
export function ForkTheirsDialog({
  repo,
  taken,
  initial,
  onClose,
}: {
  repo: Repo;
  /** The stems of the decks here already. */
  taken: ReadonlySet<string>;
  /** A login or URL to look at first, as `/?fork=` gives one. */
  initial?: string;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const [where, setWhere] = useState(initial ?? "");
  const [looked, setLooked] = useState<Looked | null>(null);
  const [deck, setDeck] = useState("");
  const [stem, setStem] = useState("");
  const look = useBusy();
  const forking = useBusy();

  const choose = (url: string, theirs: string) => {
    setDeck(theirs);
    setStem(theirs ? freeStem(theirs, ownerOf(url), taken) : "");
  };

  const lookAt = () =>
    look.run(async () => {
      const url = theirRepoUrl(where);
      if (!url)
        return "That is neither a GitHub login nor an https:// git URL.";
      const decks = await theirDecks(url);
      setLooked({ url, decks });
      choose(url, decks.kind === "lines" ? (decks.stems[0] ?? "") : "");
      return decks.kind === "none"
        ? `${url} has no decks: it is empty, or not a Magic repo.`
        : null;
    });

  const local = slugify(stem) === stem ? stem : slugify(stem);
  const owner = looked ? ownerOf(looked.url) : "";
  return (
    <Sheet label="Fork a deck from another player" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (!looked || !deck) return void (await lookAt());
          await forking.run(async () => {
            if (!local) return "The deck needs a name here.";
            if (taken.has(local))
              return `There is a deck ${local} already; pick another name.`;
            const forked = await forkTheirs(
              repo,
              { url: looked.url, deck, branch: "main" },
              local,
            );
            if (forked.kind === "taken") return forked.message;
            void navigate({ to: "/deck/$", params: { _splat: forked.stem } });
            return null;
          });
        }}
      >
        <h2>Fork a deck from another player</h2>
        <p className="hint">
          Their repo has to be public. The fork keeps their history and who made
          each change, and can take what they change later.
        </p>
        <label className="field">
          Their GitHub login, or the repo's git URL
          <span className="fork-where">
            <input
              name="where"
              value={where}
              placeholder="alice, or https://codeberg.org/alice/mtg.git"
              onChange={(e) => {
                setWhere(e.target.value);
                setLooked(null);
              }}
            />
            <button
              type="button"
              onClick={() => void lookAt()}
              disabled={look.busy}
            >
              {look.busy ? <Waiting>Looking…</Waiting> : "Look"}
            </button>
          </span>
        </label>
        {look.refusal && (
          <p className="refusal" role="alert">
            {look.refusal}
          </p>
        )}
        {looked?.decks.kind === "lines" && (
          <fieldset className="choice fork-decks">
            <legend>{owner}'s decks</legend>
            {looked.decks.stems.map((s) => (
              <label key={s}>
                <input
                  type="radio"
                  name="deck"
                  checked={deck === s}
                  onChange={() => choose(looked.url, s)}
                />
                {s}
              </label>
            ))}
          </fieldset>
        )}
        {looked?.decks.kind === "mainOnly" && (
          <label className="field">
            Their deck, by its file's name in decks/
            <input
              name="deck"
              value={deck}
              placeholder="lantern"
              onChange={(e) => choose(looked.url, e.target.value.trim())}
            />
          </label>
        )}
        {looked && deck && (
          <>
            <label className="field">
              Kept here as
              <input
                name="stem"
                value={stem}
                onChange={(e) => setStem(e.target.value)}
              />
            </label>
            <p className="hint field-hint">
              <code>decks/{local || "…"}/main</code>
              {local && local !== deck
                ? `, one commit on ${owner}'s that names the deck's files so`
                : ""}
            </p>
          </>
        )}
        {forking.refusal && (
          <p className="refusal" role="alert">
            {forking.refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={forking.busy || look.busy || (looked !== null && !deck)}
          >
            {forking.busy ? (
              <Waiting>Forking…</Waiting>
            ) : looked && deck ? (
              `Fork ${owner}'s ${deck}`
            ) : (
              "Look"
            )}
          </button>
        </div>
      </form>
    </Sheet>
  );
}
