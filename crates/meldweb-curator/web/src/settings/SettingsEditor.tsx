import {
  type DragEvent,
  type KeyboardEvent,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type Pins,
  PinsProvider,
  pinnedAs,
  rankPrintings,
} from "../card/preference";
import { printsByName } from "../card/prints";
import { usePrintingOptions } from "../card/usePrintingOptions";
import { Banners, SaveStatus, Toolbar, UndoRedo } from "../deck/Toolbar";
import type { SettingsRules } from "../deck.gen";
import type { GitHubApi, RepoRef } from "../github/api";
import { createSaveStore } from "../github/save";
import { SETTINGS_PATH } from "../github/settings";
import { useSave } from "../github/useSave";
import { useHistory, useUndoKeys } from "../history";
import { CloseIcon, PlusIcon } from "../ui/icons";
import { Pinned } from "./Pinned";
import { Preview, SAMPLES } from "./Preview";
import {
  type Draft,
  type DraftRule,
  draftRule,
  fileFor,
  type Loaded,
  missingDefaults,
  moveRule,
  type Pin,
  previewOf,
  problemOf,
  type RuleText,
  readSettings,
  sameRules,
  settingsCommitMessage,
  TERMS,
  type Verb,
} from "./rules";
import "../card/card.css";
import "./settings.css";

/**
 * Curator's settings page: `meldweb.toml`'s printing rules (ADR-0026), edited
 * as a ranked list beside a preview that ranks a real card's printings by
 * them as they are typed. It saves by itself, as a deck does, and holds the
 * save back while a rule does not parse.
 */
export function SettingsEditor({
  text,
  sha,
  repo,
  api,
}: {
  /** The repo's `meldweb.toml`, null when it has none. */
  text: string | null;
  sha: string | null;
  repo: RepoRef;
  api: GitHubApi;
}) {
  const [read, setRead] = useState(() => readSettings(text));
  const defaults = read.defaults;
  const [loaded, setLoaded] = useState<Loaded>(() =>
    loadedOf(text ?? "", read),
  );
  // A file this Curator cannot read is only replaced when asked to.
  const [replacing, setReplacing] = useState(false);
  const history = useHistory<Draft>(draftOf(read));
  const draft = history.present;
  const { rules, pins } = draft;
  const { undo, redo } = history;
  useUndoKeys(undo, redo);
  const [refusal, setRefusal] = useState<string | null>(null);

  const [store] = useState(() =>
    createSaveStore({
      api,
      repo,
      path: SETTINGS_PATH,
      text: text ?? "",
      sha,
      deckText: { commitMessage: settingsCommitMessage },
    }),
  );
  const save = useSave(store);
  const editable = read.kind === "read" || replacing;
  const file = useMemo(
    () => (editable ? fileFor(draft, loaded) : null),
    [editable, draft, loaded],
  );
  useEffect(() => {
    if (file !== null) store.edit(file);
  }, [store, file]);

  const problems = useMemo(() => rules.map((r) => problemOf(r.query)), [rules]);
  const broken = problems.flatMap((p, i) => (p ? [i + 1] : []));

  // Typing in one query is one edit to undo, not one per keystroke.
  const lastTyped = useRef<number | null>(null);
  const change = (
    next: readonly DraftRule[],
    typedIn: number | null = null,
  ) => {
    history.edit(
      { pins, rules: next },
      typedIn !== null && lastTyped.current === typedIn,
    );
    lastTyped.current = typedIn;
  };
  const setRule = (id: number, patch: Partial<RuleText>) =>
    change(
      rules.map((r) => (r.id === id ? { ...r, ...patch } : r)),
      patch.query !== undefined ? id : null,
    );

  const [focusId, setFocusId] = useState<number | null>(null);
  const list = useRef<HTMLOListElement>(null);
  useEffect(() => {
    if (focusId === null) return;
    list.current
      ?.querySelector<HTMLInputElement>(`[data-rule="${focusId}"] input`)
      ?.focus();
    setFocusId(null);
  }, [focusId]);

  const add = (rule: RuleText, at = rules.length) => {
    const fresh = draftRule(rule);
    change([...rules.slice(0, at), fresh, ...rules.slice(at)]);
    setFocusId(fresh.id);
  };
  const remove = (id: number) => change(rules.filter((r) => r.id !== id));
  const move = (from: number, to: number) => {
    if (to < 0 || to >= rules.length || from === to) return;
    change(moveRule(rules, from, to));
  };

  // The preview ranks one card's printings by the rules that read so far.
  const [card, setCard] = useState<string>(SAMPLES[0]);
  const options = usePrintingOptions(printsByName(card));
  const preview = useMemo(() => previewOf(draft), [draft]);
  const ranked = useMemo(
    () =>
      options.status === "done"
        ? rankPrintings(preview.text, options.printings)
        : null,
    [options, preview.text],
  );
  const hits = rules.map((_, i) => {
    const k = preview.at[i] ?? -1;
    return ranked?.kind === "ranked" && k >= 0 ? (ranked.hits[k] ?? 0) : null;
  });
  const setPins = (next: readonly Pin[]) => {
    history.edit({ pins: next, rules });
    lastTyped.current = null;
  };
  const pinsHere: Pins = {
    pins,
    error: null,
    toggle: (p) =>
      setPins(
        pinnedAs(pins, p)
          ? pins.filter((x) => !samePin(x, p))
          : [
              { name: p.name, set: p.set, num: p.num },
              ...pins.filter(
                (x) => x.name.toLowerCase() !== p.name.toLowerCase(),
              ),
            ],
      ),
  };
  const total = ranked?.ranked.length ?? 0;
  const [lit, setLit] = useState<number | null>(null);

  const reload = async () => {
    try {
      const fresh = await store.reload();
      const next = readSettings(fresh);
      setRead(next);
      setLoaded(loadedOf(fresh, next));
      history.reset(draftOf(next));
      setRefusal(null);
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  const missing = missingDefaults(rules, defaults);

  return (
    <main>
      <Toolbar
        name="Settings"
        status={<SaveStatus save={save} path={SETTINGS_PATH} />}
        history={
          <UndoRedo
            onUndo={undo}
            onRedo={redo}
            canUndo={history.past.length > 0}
            canRedo={history.future.length > 0}
          />
        }
      />
      <Banners
        save={save}
        refusal={refusal}
        onReload={() => void reload()}
        onOverwrite={() => void store.overwrite()}
        onDismiss={() => setRefusal(null)}
      />
      <div className="settings">
        <div className="settings-main">
          {editable && (
            <Pinned
              pins={pins}
              onUnpin={(p) => setPins(pins.filter((x) => !samePin(x, p)))}
              onPreview={setCard}
            />
          )}
          <section className="settings-rules" aria-labelledby="printing-rules">
            <h2 id="printing-rules">Every other card</h2>
            <p className="settings-lede">
              For a card with no favourite, these rules decide which printing
              adding it gives you, and the order you pick from. Each rule is a
              Scryfall query about one printing. Rules apply top to bottom: the
              first one that tells two printings apart decides, and each one
              below only breaks ties. Printings no rule tells apart go newest
              first.
            </p>
            {read.kind === "refused" && !replacing ? (
              <div className="settings-unreadable">
                <p className="refusal" role="alert">
                  {read.message}
                </p>
                <details className="source" open>
                  <summary>{SETTINGS_PATH} as it is</summary>
                  <pre>{text}</pre>
                </details>
                <button
                  type="button"
                  className="primary"
                  onClick={() => setReplacing(true)}
                >
                  Replace it with the default rules
                </button>
              </div>
            ) : (
              <>
                <p className="settings-source">
                  {read.kind === "read" && read.declared ? (
                    <>
                      Saved in <code>{SETTINGS_PATH}</code> at the root of{" "}
                      <code>
                        {repo.owner}/{repo.name}
                      </code>
                      , as a commit, like your decks.
                    </>
                  ) : (
                    <>
                      <code>
                        {repo.owner}/{repo.name}
                      </code>{" "}
                      has no <code>{SETTINGS_PATH}</code> yet, so these are the
                      default rules. Your first change commits the file.
                    </>
                  )}
                </p>
                <RuleList
                  rules={rules}
                  problems={problems}
                  listRef={list}
                  hits={hits}
                  total={total}
                  card={card}
                  onVerb={(id, verb) => setRule(id, { verb })}
                  onQuery={(id, query) => setRule(id, { query })}
                  onRemove={remove}
                  onMove={move}
                  onAddAfter={(i) => add({ verb: "avoid", query: "" }, i + 1)}
                  onLight={setLit}
                />
                {rules.length === 0 && (
                  <p className="settings-empty">
                    No rules: every card's printings are listed newest first.
                  </p>
                )}
                <div className="rule-actions">
                  <button
                    type="button"
                    onClick={() => add({ verb: "avoid", query: "" })}
                  >
                    <PlusIcon />
                    Add rule
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    disabled={sameRules(rules, defaults)}
                    onClick={() => change(defaults.map(draftRule))}
                    title="Undo takes this back"
                  >
                    Reset to the default rules
                  </button>
                  {broken.length > 0 && (
                    <span className="rule-held" role="status">
                      Not saved until rule{broken.length === 1 ? "" : "s"}{" "}
                      {broken.join(", ")}{" "}
                      {broken.length === 1 ? "reads" : "read"} as a query.
                    </span>
                  )}
                </div>
                {missing.length > 0 && (
                  <div className="rule-suggestions">
                    <span>Default rules you don't have:</span>
                    {missing.map((r) => (
                      <button
                        key={`${r.verb}:${r.query}`}
                        type="button"
                        className={`rule-chip ${r.verb}`}
                        title="Add it at the bottom"
                        onClick={() => add(r)}
                      >
                        <PlusIcon />
                        {r.verb} <code>{r.query}</code>
                      </button>
                    ))}
                  </div>
                )}
                <CheatSheet />
              </>
            )}
          </section>
        </div>
        <PinsProvider pins={editable ? pinsHere : null}>
          <Preview
            card={card}
            onCard={setCard}
            options={options}
            ranked={ranked}
            skipped={preview.at.filter((k) => k < 0).length}
            // A rule that stopped reading while pointed at lights nothing.
            lit={lit === null || problems[lit] ? null : (rules[lit] ?? null)}
          />
        </PinsProvider>
      </div>
    </main>
  );
}

/** The ranked rules, each a row to retype, flip, move, drag or drop. */
function RuleList({
  rules,
  problems,
  listRef,
  hits,
  total,
  card,
  onVerb,
  onQuery,
  onRemove,
  onMove,
  onAddAfter,
  onLight,
}: {
  rules: readonly DraftRule[];
  problems: readonly (string | null)[];
  listRef: RefObject<HTMLOListElement | null>;
  /** How many of the preview card's printings each rule matches, when known. */
  hits: readonly (number | null)[];
  total: number;
  card: string;
  onVerb: (id: number, verb: Verb) => void;
  onQuery: (id: number, query: string) => void;
  onRemove: (id: number) => void;
  onMove: (from: number, to: number) => void;
  onAddAfter: (index: number) => void;
  /** The rule whose printings the preview should pick out, or null. */
  onLight: (index: number | null) => void;
}) {
  // Only the grip starts a drag, so selecting text in a query still works.
  const [armed, setArmed] = useState<number | null>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);

  const onKey = (e: KeyboardEvent, i: number) => {
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      const to = e.key === "ArrowUp" ? i - 1 : i + 1;
      onMove(i, to);
      // The row moved; its input keeps focus because its key did not change.
      const input = e.currentTarget as HTMLInputElement;
      requestAnimationFrame(() => input.focus());
    } else if (e.key === "Enter") {
      e.preventDefault();
      onAddAfter(i);
    }
  };

  const dropAt = (e: DragEvent, i: number) => {
    e.preventDefault();
    if (dragging !== null) {
      // Dropped on the lower half of a row, the rule goes below it.
      const box = e.currentTarget.getBoundingClientRect();
      const below = e.clientY > box.top + box.height / 2;
      let to = below ? i + 1 : i;
      if (dragging < to) to -= 1;
      onMove(dragging, to);
    }
    setDragging(null);
    setOver(null);
    setArmed(null);
  };

  return (
    <ol className="rule-list" ref={listRef}>
      {rules.map((rule, i) => {
        const problem = problems[i] ?? null;
        const problemId = `rule-problem-${rule.id}`;
        const classes = [
          "rule-row",
          rule.verb,
          dragging === i ? "dragging" : "",
          over === i && dragging !== null && dragging !== i
            ? "drop-target"
            : "",
        ]
          .filter(Boolean)
          .join(" ");
        return (
          <li
            key={rule.id}
            className={classes}
            data-rule={rule.id}
            draggable={armed === rule.id}
            onDragStart={(e) => {
              e.dataTransfer.effectAllowed = "move";
              e.dataTransfer.setData("text/plain", rule.query);
              setDragging(i);
            }}
            onDragEnd={() => {
              setDragging(null);
              setOver(null);
              setArmed(null);
            }}
            onDragOver={(e) => {
              if (dragging === null) return;
              e.preventDefault();
              setOver(i);
            }}
            onDrop={(e) => dropAt(e, i)}
            onMouseEnter={() => onLight(problem ? null : i)}
            onMouseLeave={() => onLight(null)}
          >
            <span
              className="rule-grip"
              title="Drag to reorder (or Alt+↑ / Alt+↓ in the query)"
              aria-hidden="true"
              onPointerDown={() => setArmed(rule.id)}
              onPointerUp={() => setArmed(null)}
            >
              ⠿
            </span>
            <span className="rule-rank" aria-hidden="true">
              {i + 1}
            </span>
            {/* biome-ignore lint/a11y/useSemanticElements: two pressed buttons read better than radios here */}
            <div
              className="verb-toggle"
              role="group"
              aria-label={`Rule ${i + 1}: prefer or avoid`}
            >
              <button
                type="button"
                className="verb prefer"
                aria-pressed={rule.verb === "prefer"}
                onClick={() => onVerb(rule.id, "prefer")}
                title="Printings this matches go first"
              >
                ↑ Prefer
              </button>
              <button
                type="button"
                className="verb avoid"
                aria-pressed={rule.verb === "avoid"}
                onClick={() => onVerb(rule.id, "avoid")}
                title="Printings this matches go last"
              >
                ↓ Avoid
              </button>
            </div>
            <input
              className="rule-query"
              value={rule.query}
              placeholder="a Scryfall query, e.g. is:fullart"
              aria-label={`Rule ${i + 1} query`}
              aria-invalid={problem !== null && rule.query !== ""}
              aria-describedby={problem ? problemId : undefined}
              spellCheck={false}
              autoCapitalize="off"
              autoComplete="off"
              onChange={(e) => onQuery(rule.id, e.target.value)}
              onKeyDown={(e) => onKey(e, i)}
              onFocus={() => onLight(problem ? null : i)}
              onBlur={() => onLight(null)}
            />
            <span
              className={hits[i] === 0 ? "rule-hits none" : "rule-hits"}
              title={
                hits[i] == null
                  ? undefined
                  : `Matches ${hits[i]} of the ${total} printings of ${card} in the preview`
              }
            >
              {hits[i] == null ? "" : `${hits[i]}/${total}`}
            </span>
            <div className="rule-tools">
              <button
                type="button"
                className="icon ghost small"
                aria-label={`Move rule ${i + 1} up`}
                title="Move up (Alt+↑)"
                disabled={i === 0}
                onClick={() => onMove(i, i - 1)}
              >
                ↑
              </button>
              <button
                type="button"
                className="icon ghost small"
                aria-label={`Move rule ${i + 1} down`}
                title="Move down (Alt+↓)"
                disabled={i === rules.length - 1}
                onClick={() => onMove(i, i + 1)}
              >
                ↓
              </button>
              <button
                type="button"
                className="icon ghost small danger"
                aria-label={`Remove rule ${i + 1}`}
                title="Remove"
                onClick={() => onRemove(rule.id)}
              >
                <CloseIcon />
              </button>
            </div>
            {problem && rule.query !== "" && (
              <p className="rule-problem" id={problemId}>
                {problem}
              </p>
            )}
          </li>
        );
      })}
    </ol>
  );
}

function CheatSheet() {
  return (
    <details className="rule-help">
      <summary>Printing terms</summary>
      <p>
        Combine terms with spaces (and), <code>or</code>, <code>-</code> (not)
        and parentheses, as on Scryfall. Card terms work too:{" "}
        <code>t:basic</code> scopes a rule to basic lands.
      </p>
      <dl>
        {TERMS.map(([term, what]) => (
          <div key={term}>
            <dt>
              <code>{term}</code>
            </dt>
            <dd>{what}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}

/** The draft a read file starts the page at: its own, or the defaults. */
function draftOf(read: SettingsRules): Draft {
  return read.kind === "read"
    ? { pins: read.pins, rules: read.rules.map(draftRule) }
    : { pins: [], rules: read.defaults.map(draftRule) };
}

function loadedOf(text: string, read: SettingsRules): Loaded {
  return read.kind === "read"
    ? { text, pins: read.pins, rules: read.rules }
    : // Nothing matches a file that does not read, so any start replaces it.
      { text, pins: [], rules: [] };
}

function samePin(a: Pin, b: Pick<Pin, "name" | "set" | "num">): boolean {
  return pinnedAs([a], b);
}
