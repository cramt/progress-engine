// Generated from crates/meldweb-curator/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p meldweb-wasm rewrites it.

export type DeckBlame =
  | { kind: "blamed"; at: string; cards: CardBlame[] }
  | { kind: "noDeck" };

/**
 * One card line of the deck blamed.
 */
export interface CardBlame {
  /**
   * The line's index among the deck's cards.
   */
  index: number;
  /**
   * What its last change did to it, as that commit's message says it:
   * `+1 Sol Ring`, `Sol Ring: 1 → 2`.
   */
  changes: string[];
  blame: Blame;
}

/**
 * When a card came into the deck and when it last changed.
 */
export type Blame =
  | { kind: "addedUnsaved" }
  | { kind: "changedUnsaved"; added: BlameCommit }
  | { kind: "added"; added: BlameCommit }
  | { kind: "changed"; added: BlameCommit; changed: BlameCommit };

/**
 * One commit, as a card's blame names it.
 */
export interface BlameCommit {
  commit: string;
  /**
   * The stem of the deck the card was in at that commit: this deck's, or
   * for a card unchanged since a fork, the deck it was forked from.
   */
  deck: string;
  author: string;
  email: string;
  /**
   * The author's time, in seconds since the Unix epoch, as `git blame`
   * dates a line.
   */
  authoredAt: number;
  /**
   * Without the newline git ends a message with.
   */
  message: string;
}

export type MergeDone =
  | { kind: "merged"; commit: string; message?: string }
  | { kind: "upToDate" }
  | { kind: "stopped"; message: string };

/**
 * What merging one of a deck's branches into another would do. `into` and
 * `from` are the two tips it was worked out from, which the merge itself
 * is guarded by; `before` and `after` are the deck's file on the branch
 * merged into, now and once merged, absent where the branch holds none.
 */
export type MergePlan =
  | { kind: "upToDate" }
  | { kind: "fastForward"; into: string; from: string; before?: string; after?: string }
  | { kind: "merge"; into: string; from: string; before?: string; after?: string; message: string }
  | { kind: "conflict"; into: string; from: string; hunks: ConflictHunk[]; unreadable: MergeRefusal[] }
  | { kind: "unrelated" };

/**
 * A file whose lines merged into something Progress Engine does not read.
 */
export interface MergeRefusal {
  path: string;
  reason: string;
}

/**
 * One stretch of a file that both branches changed, differently.
 */
export interface ConflictHunk {
  path: string;
  /**
   * The lines as the merge base had them.
   */
  base: string[];
  /**
   * As the branch merged into has them.
   */
  ours: string[];
  /**
   * As the branch merged from has them.
   */
  theirs: string[];
}

export type BranchDeleted =
  | { kind: "deleted" }
  | { kind: "conflict"; message: string };

export type BranchMade =
  | { kind: "made"; branch: BranchFacts }
  | { kind: "conflict"; message: string };

/**
 * One of a deck's branches, as the picker lists it.
 */
export interface BranchFacts {
  /**
   * Its own name: `budget` for `decks/lantern/budget`.
   */
  name: string;
  /**
   * Its tip.
   */
  commit: string;
  /**
   * When its tip was committed, in seconds since the Unix epoch.
   */
  committedAt: number;
  /**
   * Commits on it that the deck's `main` lacks: what merging it brings.
   * Deleting a branch with any loses them.
   */
  unmerged: number;
  /**
   * Commits on `main` that it lacks: what updating it from `main` brings.
   */
  behind: number;
}

/**
 * A deck's branches, `main` first and then the newest first.
 */
export interface DeckBranches {
  branches: BranchFacts[];
}

/**
 * What migrating the repo to the next version came to.
 */
export interface MigratedFacts {
  version: number;
  decks: number;
  snapshots: number;
}

export type DeckDeleted =
  | { kind: "deleted" }
  | { kind: "conflict"; message: string };

export interface DeckLineList {
  decks: DeckLine[];
}

/**
 * One deck as the deck list reads it: its stem, the text on its `main`,
 * and the deck it was forked from, read from its history (ADR-0041).
 */
export interface DeckLine {
  stem: string;
  /**
   * The tip of `decks/<stem>/main`.
   */
  commit: string;
  /**
   * `<stem>.deck.toml` there; absent when the line holds none.
   */
  text?: string;
  /**
   * Its blob id, which a later write sends back as its guard.
   */
  sha?: string;
  parent?: string;
}

/**
 * What a push came to, ref by ref.
 */
export interface PushOutcome {
  updated: string[];
  rejected: Refused[];
}

export interface Refused {
  ref: string;
  reason: string;
}

/**
 * What bringing the remote's branches into these came to, all of them
 * together; see `meldweb_git::Integrated`. A path is named as it is on its
 * branch, and a deck's with its branch before it:
 * `decks/lantern/main: lantern.deck.toml`.
 */
export type Synced =
  | { kind: "remoteEmpty" }
  | { kind: "upToDate" }
  | { kind: "ahead" }
  | { kind: "fastForwarded"; to: string }
  | { kind: "adopted"; to: string }
  | { kind: "merged"; commit: string; taken: string[] }
  | { kind: "conflict"; paths: string[] }
  | { kind: "unrelated" };

/**
 * The remote, and what is waiting to go to it or come from it, as the
 * settings page shows them.
 */
export interface RemoteFacts {
  /**
   * `origin`'s URL; absent when the repo has no remote.
   */
  url?: string;
  /**
   * Commits on the branches here that the remote's lack, as last fetched.
   */
  ahead: number;
  /**
   * Commits on the remote's branches, as last fetched, that none here holds.
   */
  behind: number;
  /**
   * Branches (decks, mostly) to make, move or delete on the remote.
   */
  branches: number;
  /**
   * Snapshots to make, move or delete on the remote.
   */
  tags: number;
}

/**
 * What a write to the default branch came to.
 */
export type Wrote =
  | { kind: "committed"; sha?: string; commit: string }
  | { kind: "conflict"; message: string };

export type Tagged =
  | { kind: "tagged"; snapshot: TagFacts }
  | { kind: "conflict"; message: string };

/**
 * An annotated tag: a snapshot (ADR-0024).
 */
export interface TagFacts {
  tag: string;
  /**
   * The tag's message, without the newline git ends it with.
   */
  label: string;
  commit: string;
  /**
   * The tagger's time in seconds since the Unix epoch, when it has one.
   */
  taggedAt?: number;
}

export interface Snapshots {
  snapshots: TagFacts[];
}

export type RevisionRead =
  | { kind: "found"; revision: CommitFacts }
  | { kind: "missing" };

/**
 * One commit, as a deck's history lists it.
 */
export interface CommitFacts {
  commit: string;
  /**
   * Without the newline git ends a message with.
   */
  message: string;
  /**
   * The committer's time, in seconds since the Unix epoch.
   */
  committedAt: number;
  /**
   * The author's name.
   */
  author: string;
}

export interface Revisions {
  revisions: CommitFacts[];
}

export interface Listing {
  entries: ListedEntry[];
}

export interface ListedEntry {
  name: string;
  path: string;
  sha: string;
  type: EntryType;
}

export type EntryType = "file" | "dir" | "symlink" | "submodule";

export type FileRead =
  | { kind: "file"; text: string; sha: string }
  | { kind: "noFile" };

/**
 * Who wrote a commit or a tag, and when, as the page knows it: the identity
 * set in this browser (ADR-0041) and its clock.
 */
export interface Who {
  name: string;
  email: string;
  /**
   * Seconds since the Unix epoch.
   */
  seconds: number;
  /**
   * Minutes east of UTC, the negation of `getTimezoneOffset()`.
   */
  offsetMinutes: number;
}

/**
 * What the worker must write through before it answers, in order: objects
 * before the refs that name them, as `meldweb-git` wrote them.
 */
export interface Journal {
  steps: Step[];
}

/**
 * One thing an operation did to the git dir, for the worker to repeat.
 */
export type Step =
  | { kind: "write"; path: string }
  | { kind: "remove"; path: string }
  | { kind: "dir"; path: string };

export type ToPrint =
  | { kind: "toPrint"; lines: ProxyLine[]; basics: number; owned: number }
  | { kind: "refused"; message: string };

/**
 * One deck line's proxies.
 */
export interface ProxyLine {
  /**
   * The deck line, a `DeckCard.index`.
   */
  index: number;
  qty: number;
}

/**
 * How a sheet lays cards out, as `metamorph`'s flags say it.
 */
export interface SheetLayout {
  paper: Paper;
  /**
   * Millimetres between cards.
   */
  gapMm: number;
  /**
   * Millimetres kept clear at the paper's edge.
   */
  marginMm: number;
  cutMarks: boolean;
}

/**
 * Portrait.
 */
export type Paper = "a4" | "a3" | "letter" | "legal";

/**
 * What [`store_plan`] decided, for the worker to carry out.
 */
export type StoreStep =
  | { kind: "current"; meta?: string }
  | { kind: "refused"; message: string }
  | { kind: "build"; file: string };

export type SearchAnswer =
  | { kind: "page"; cards: Found[]; total: number }
  | { kind: "refused"; message: string };

/**
 * One printing, with what any view of it asks.
 */
export interface Found {
  /**
   * Scryfall's id for the printing.
   */
  id: string;
  oracleId?: string;
  name: string;
  /**
   * Lowercased, as the deck names it.
   */
  set: string;
  num: string;
  setName: string;
  /**
   * `YYYY-MM-DD`.
   */
  released: string;
  /**
   * The front's `normal` picture.
   */
  image?: string;
  small?: string;
  colorIdentity: string[];
  /**
   * The whole card's type line, both faces' where it has two.
   */
  typeLine: string;
  /**
   * The front face's alone: `Battle — Siege` for an Invasion.
   */
  frontTypeLine: string;
  manaCost: string;
  /**
   * Scryfall's search for every printing of the card, which
   * [`ScryfallCopy::prints`] also answers.
   */
  prints?: string;
  turn?: Turn;
  back?: Face;
  finishes: string[];
  prices: CardPrices;
}

/**
 * A printing's prices, by currency and finish, where Scryfall knows them.
 */
export interface CardPrices {
  eur?: FinishPrices;
  usd?: FinishPrices;
}

/**
 * What one copy of one card sells for, by finish.
 */
export interface FinishPrices {
  nonfoil?: number;
  foil?: number;
  etched?: number;
}

/**
 * A printing's other face, as it is shown.
 */
export interface Face {
  image: string;
  turn: Turn;
}

/**
 * How a card's picture is turned to be read.
 */
export type Turn = "upright" | "sideways" | "upside-down";

/**
 * A printing beside its card object, which a printing preference reads.
 */
export interface PrintingFacts {
  printing: Found;
  facts: BulkCard;
}

/**
 * One card object as Scryfall's bulk data writes it.
 *
 * Every field is optional, because a field this crate reads is a field
 * Scryfall may one day stop printing, and a sync that dies on the whole file
 * over one absent key is worse than one that reports what it could not find.
 * `name` is the exception: a record with no name cannot be keyed, so it is not
 * a card as far as this crate is concerned.
 */
export interface BulkCard {
  name: string;
  oracle_id?: string;
  lang?: string;
  layout?: string;
  type_line?: string;
  oracle_text?: string;
  mana_cost?: string;
  cmc?: number;
  colors?: string[];
  color_indicator?: string[];
  color_identity?: string[];
  produced_mana?: string[];
  keywords?: string[];
  power?: string;
  toughness?: string;
  loyalty?: string;
  defense?: string;
  rarity?: string;
  set?: string;
  legalities?: Record<string, string>;
  game_changer?: boolean;
  reserved?: boolean;
  card_faces?: BulkFace[];
  set_type?: string;
  collector_number?: string;
  released_at?: string;
  frame?: string;
  frame_effects?: string[];
  border_color?: string;
  full_art?: boolean;
  textless?: boolean;
  digital?: boolean;
  promo?: boolean;
  reprint?: boolean;
  oversized?: boolean;
  promo_types?: string[];
  games?: string[];
  flavor_name?: string;
}

/**
 * One face of a multi-faced card, as Scryfall writes it.
 */
export interface BulkFace {
  name?: string;
  type_line?: string;
  oracle_text?: string;
  mana_cost?: string;
  colors?: string[];
  color_indicator?: string[];
  power?: string;
  toughness?: string;
  loyalty?: string;
  defense?: string;
  oracle_id?: string;
  image_uris?: ImageUris;
}

/**
 * A face's pictures, of which only the sizes Meldweb shows are read.
 */
export interface ImageUris {
  normal?: string;
  small?: string;
}

/**
 * A card asked for.
 */
export type Wanted =
  | { kind: "printing"; set: string; num: string }
  | { kind: "name"; name: string }
  | { kind: "id"; id: string }
  | { kind: "inSet"; name: string; set: string };

/**
 * `meldweb.toml` as the settings page edits it: its rules, each query unread
 * so a bad one can be fixed in place, and the default rules beside them.
 */
export type SettingsRules =
  | { kind: "read"; pins: Pin[]; rules: RuleText[]; decks: string[]; friends: string[]; declared: boolean; defaults: RuleText[] }
  | { kind: "refused"; message: string; defaults: RuleText[] };

/**
 * One rule, as the file wrote it.
 */
export interface RuleText {
  verb: Verb;
  query: string;
}

export type Verb = "prefer" | "avoid";

/**
 * One card's own printing: a `prefer` rule naming exactly that card and
 * printing, `!"Sol Ring" set:c21 cn:263`. It is a rule like any other to the
 * ranking, written above the rest so it beats them all for its card; the
 * settings page shows it as the card it is, not as a query.
 */
export interface Pin {
  name: string;
  set: string;
  num: string;
}

export type Ranked =
  | { kind: "ranked"; rules: RuleText[]; declared: boolean; pins: number[]; order: RankedPrinting[] }
  | { kind: "refused"; message: string };

/**
 * One printing in [`Ranked`]'s order: its index in what was ranked, and
 * which rules it matched.
 */
export interface RankedPrinting {
  index: number;
  matched: number[];
}

export type Compared =
  | { kind: "diff"; changes: DeckChange[] }
  | { kind: "refused"; message: string };

/**
 * One change between two decks, as `chip_decklist::diff` finds it. A card
 * line is named by its index in the `before` deck, the `after` deck, or
 * both; `text` is the line a commit message says the change with.
 */
export type DeckChange =
  | { kind: "add"; after: number; text: string }
  | { kind: "remove"; before: number; text: string }
  | { kind: "qty"; before: number; after: number; text: string }
  | { kind: "move"; before: number; after: number; text: string }
  | { kind: "printing"; before: number; after: number; text: string }
  | { kind: "finish"; before: number; after: number; text: string }
  | { kind: "declare"; category: string; text: string }
  | { kind: "undeclare"; category: string; text: string }
  | { kind: "retype"; category: string; text: string }
  | { kind: "rename"; text: string }
  | { kind: "format"; text: string }
  | { kind: "cover"; text: string }
  | { kind: "description"; text: string };

/**
 * A friend's collection as the page loaded it.
 */
export interface FriendFile {
  who: string;
  text: string;
}

export type ParsedSupply =
  | { kind: "supply"; place?: string; lines: SupplyLine[]; short: SupplyShort[]; extra: SupplyExtra[]; unread: FriendUnread[] }
  | { kind: "refused"; message: string };

/**
 * A friend's collection that could not be read, and why.
 */
export interface FriendUnread {
  who: string;
  message: string;
}

/**
 * Copies in the deck's place no line takes.
 */
export interface SupplyExtra {
  line: number;
  name: string;
  card: CardRef;
  finish: Finish;
  qty: number;
  /**
   * The deck lists the card, in another printing or fewer copies.
   */
  listed: boolean;
}

export type Finish = "nonfoil" | "foil" | "etched";

export type CardRef =
  | { kind: "printing"; set: string; num: string }
  | { kind: "name"; name: string };

/**
 * A card the collection has too few copies of for the deck.
 */
export interface SupplyShort {
  name: string;
  /**
   * The first deck line holding the card, a `DeckCard.index`.
   */
  line: number;
  short: number;
  basic: boolean;
  friends: FriendHas[];
}

/**
 * A friend's copies of a card the deck is short of.
 */
export interface FriendHas {
  who: string;
  pulls: TradePull[];
}

/**
 * Copies of one of their collection's lines to bring to a trade.
 */
export interface TradePull {
  /**
   * Their place the copies are in; absent for unsorted.
   */
  at?: string;
  /**
   * The place is one of their decks.
   */
  inDeck: boolean;
  card: CardRef;
  finish: Finish;
  qty: number;
}

/**
 * A deck line needing copies, and the collection's copies it gets.
 */
export interface SupplyLine {
  /**
   * The deck line, a `DeckCard.index`.
   */
  index: number;
  name: string;
  need: number;
  /**
   * Exact copies first, each half nearest first.
   */
  copies: SupplyCopies[];
}

/**
 * Copies of one collection line that make up part of a deck line.
 */
export interface SupplyCopies {
  /**
   * The collection line, an `OwnedCard.index`.
   */
  line: number;
  /**
   * Its place; absent for unsorted.
   */
  at?: string;
  whereabouts: Whereabouts;
  card: CardRef;
  finish: Finish;
  qty: number;
  /**
   * The printing and finish the deck line names, rather than a stand-in.
   */
  exact: boolean;
}

/**
 * Where an owned copy is, seen from the deck being built.
 */
export type Whereabouts = "thisDeck" | "loose" | "otherDeck";

export type ParsedTrade =
  | { kind: "trade"; offers: TradeOffer[]; unread: Unread[] }
  | { kind: "refused"; message: string };

/**
 * A deck file the derived list could not read, and why.
 */
export interface Unread {
  stem: string;
  message: string;
}

/**
 * A card the wanted list is short of that their collection holds.
 */
export interface TradeOffer {
  name: string;
  /**
   * Copies still wanted, by hand and by the decks.
   */
  short: number;
  pulls: TradePull[];
}

/**
 * A deck as the page loaded it: its stem, the text on its `main`, and the
 * deck it was forked from, which the deck list read from its history.
 */
export interface DeckFile {
  stem: string;
  text: string;
  parent?: string;
}

export type ParsedWanted =
  | { kind: "wanted"; deckCopies: DeckCopies; cards: WantedCard[]; missing: MissingCard[]; collection?: string; unread: Unread[] }
  | { kind: "refused"; message: string };

/**
 * A card the decks hold more copies of than the collection does.
 */
export interface MissingCard {
  name: string;
  missing: number;
  owned: number;
  decks: DeckNeed[];
}

/**
 * A deck holding a card the collection is short of.
 */
export interface DeckNeed {
  stem: string;
  /**
   * The deck's `name`, or its stem when it has none.
   */
  name: string;
  qty: number;
}

/**
 * One line of the wanted list, and how many of it the collection holds.
 */
export interface WantedCard {
  /**
   * Position in the file's `cards` list, 0-based: how an edit finds it.
   */
  index: number;
  card: CardRef;
  qty: number;
  finish: Finish;
  /**
   * Copies owned of any printing of the card.
   */
  owned: number;
}

/**
 * How the decks count toward the wanted list (ADR-0034).
 */
export type DeckCopies = "each" | "shared";

export interface CollectionImported {
  text: string;
  /**
   * Rows kept by name where the file named a printing, and why.
   */
  notes: ImportNote[];
  /**
   * Rows that did not go in: the file's, and those Scryfall named nothing.
   */
  unreadable: Unreadable[];
}

/**
 * A line of pasted text or an exported file that an import could not carry
 * over whole.
 */
export interface Unreadable {
  /**
   * 1-based, counting every line of the text.
   */
  line: number;
  text: string;
  reason: string;
}

/**
 * A row that went in, but not as its file named it.
 */
export interface ImportNote {
  line: number;
  reason: string;
}

/**
 * A card Scryfall answered an [`Ask`] with.
 */
export interface ScryfallCard {
  id: string;
  set: string;
  num: string;
  name: string;
}

/**
 * Another app's export, read but not yet resolved against Scryfall.
 */
export type CollectionExport =
  | { kind: "export"; source: string; rows: number; copies: number; places: string[]; unplaced: boolean; asks: Ask[]; unreadable: Unreadable[]; dropped: Dropped[]; skipped: Skipped[] }
  | { kind: "refused"; message: string };

export interface Skipped {
  rows: number;
  reason: string;
}

/**
 * A column an import leaves behind, and how many rows gave it a value.
 */
export interface Dropped {
  column: string;
  rows: number;
}

/**
 * What Scryfall is asked about an export's rows: a printing by its id, or
 * by set and collector number.
 */
export type Ask =
  | { kind: "id"; id: string }
  | { kind: "printing"; set: string; num: string };

export type ParsedCollection =
  | { kind: "collection"; places: Place[]; cards: OwnedCard[]; total: number }
  | { kind: "refused"; message: string };

/**
 * One line of the collection (ADR-0023).
 */
export interface OwnedCard {
  /**
   * Position in the file's `cards` list, 0-based: how an edit finds it.
   */
  index: number;
  card: CardRef;
  qty: number;
  finish: Finish;
  /**
   * The place the copies are in; absent for unsorted.
   */
  at?: string;
}

export interface Place {
  name: string;
  /**
   * The stem of the deck this place is, when it is one.
   */
  deck?: string;
}

export type Imported =
  | { kind: "imported"; toml: string; unreadable: Unreadable[]; setOnly: SetOnly[] }
  | { kind: "refused"; message: string };

/**
 * A card named by name whose Archidekt line also gave a set.
 */
export interface SetOnly {
  /**
   * Into the deck's cards, as the edits index them.
   */
  index: number;
  line: number;
  text: string;
  name: string;
  set: string;
}

/**
 * An edit's next text, and where each card of the old text is in it:
 * `lines[i]` is where old card `i` went.
 */
export interface Edited {
  text: string;
  lines: Line[];
}

/**
 * Where one card of the text an edit started from is in the text it made.
 */
export type Line =
  | { kind: "at"; index: number }
  | { kind: "gone" };

/**
 * A card as the user reached it: its line, and the category of the stack it
 * was reached in, absent for no category.
 */
export interface Target {
  index: number;
  from?: string;
}

/**
 * One action on every card it is applied to.
 */
export type DeckEdit =
  | { kind: "increase" }
  | { kind: "decrease" }
  | { kind: "remove" }
  | { kind: "automatic" }
  | { kind: "commander" }
  | { kind: "move"; to: Dest; secondary: boolean };

/**
 * Where a moved card goes.
 */
export type Dest =
  | { kind: "category"; name: string }
  | { kind: "board"; board: Board };

/**
 * A board a card can be put on at a keystroke or by the drag strip.
 */
export type Board = "maybeboard" | "sideboard";

/**
 * What an add did: the next text, the line holding the card now, and
 * whether that line is new.
 */
export interface Added {
  text: string;
  line: number;
  made: boolean;
}

/**
 * Where a card added to a deck goes.
 */
export type AddTo =
  | { kind: "automatic" }
  | { kind: "categories"; categories: string[] };

/**
 * A card to add: a [`CardRef`], and for a printing optionally the card's
 * name, written beside the new line as its comment the way an import does.
 */
export type NewCard =
  | { kind: "printing"; set: string; num: string; name?: string }
  | { kind: "name"; name: string };

export type Parsed =
  | { kind: "deck"; name?: string; format?: string; cover?: Printing; description?: string; categories: Category[]; cards: Card[]; total: number }
  | { kind: "refused"; message: string };

export interface Card {
  /**
   * Position in the file's `cards` list, 0-based: how an edit finds it.
   */
  index: number;
  card: CardRef;
  qty: number;
  finish: Finish;
  categories: string[];
  /**
   * The deepest type among its categories, `"in-deck"` when none is typed.
   */
  place: Kind;
  /**
   * Counted toward the deck: `place` is within in-deck.
   */
  inDeck: boolean;
}

/**
 * The category type tree (ADR-0020), as the strings the file uses.
 */
export type Kind = "in-deck" | "commander" | "not-in-deck" | "sideboard" | "companion" | "maybeboard" | "attractions" | "sticker-sheet";

export interface Category {
  name: string;
  /**
   * Absent for a label that says nothing about where a card is.
   */
  kind?: Kind;
}

/**
 * A printing, `set/num` in the file.
 */
export interface Printing {
  set: string;
  num: string;
}
