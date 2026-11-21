# Insight batch {{batch}}: the writer's brief

You are the writer for batch {{batch}} of Vivisurgeon's Insight, the Oracle-text
grammar in `{{crate}}` (binary `insight`). The repo is `{{repo}}`; read its
CLAUDE.md and the crate's README.md and SEMANTICS.md before changing anything.

The `gate` and `finish` subcommands judge your commits, not your prose: what
they read is the commits, `{{handoffPath}}` and `{{batchDir}}/report.json`.

## This batch

- Milestone: **{{milestone}}**. {{milestoneRow}}
- Baseline at {{baselineHead}}: {{baseline}}
- Stop after about **{{commitCap}} commits** or **{{hours}} hours** (started
  {{startedAt}}), whichever comes first. `gate` prints where you stand.

## Order of work

1. The queued review findings below, regressions first, then the ones that
   change the game. Fix them **by pattern**: find the rule that produced the
   wrong tree and every other card it touches, not just the named card.
2. Then the milestone's failing abilities, most played first.

## Rules

- Commit subjects start with the lowercase prefix `insight: `, then make a
  claim about behaviour, naming the card: `insight: Molten Disaster's "If this spell was
  kicked, it has split second" gives split second to the spell`.
- Put **exact counts** in commit bodies: abilities and cards gained, trees
  changed. Measure them with the CLI; never estimate.
- Run `cargo fmt --all` before every commit.
- `src/walk.rs` is generated from `ast.rs`. After any `ast.rs` change, once the
  crate compiles, run `UPDATE_WALK=1 cargo test -p vivisurgeons-insight --test
  walk`, then `cargo fmt -p vivisurgeons-insight`.
- Lowering a lint count: `UPDATE_LINT=1 cargo test -p vivisurgeons-insight
  --test lint`. A lint count may never rise.
- Verify card text on Scryfall (`POST https://api.scryfall.com/cards/collection`,
  75 identifiers per request) or with `insight card <name>`, and rules on
  academyruins: `curl -s https://api.academyruins.com/cr/107.3 | jq -r .ruleText`.
  Never state card text or a rule from memory.
- When a tracked number moves, update the crate's README.md (and any doc that
  states it) in the same commit.
- A SEMANTICS question with no row is yours to settle: decide it by the
  card's Oracle text, the CR and SEMANTICS.md's principles (a tree holds the
  meaning, Oracle's shorthand undone), add the settled row to SEMANTICS.md in
  the commit that carries it out, and name it in report.json's `summary`. The
  `decision` escalations in the state dir's `escalations.md` that no row
  settles yet are yours too: settle one whenever your work touches it.
- Escalate only what is not a question of meaning: excluding a card (Un-cards,
  ante, acorn), or what the tool is for. Carry on with other work.

## The gates your commits must pass

`cargo fmt --all --check`; `cargo clippy --all-targets -- -D warnings`;
`cargo test -p vivisurgeons-insight`; round-trip failures not above
{{roundtripFailures}}; abilities parsed not below {{abilitiesOk}}; no lint kind
above its baseline ({{lintBaseline}}); and no ability that parsed at the
baseline may stop parsing (`insight snapshot` against
`{{baselineSnapshot}}`, compared by card and text). The git gates: on
`{{branch}}`, clean, `insight: ` subjects, no AI attribution, nothing of the
batch's on origin, `decisions.md` unchanged, no golden file's pinned card count
moved.

`cargo test` holds every tree in `golden/` and `snapshot/` to what it was.
When a rule moves trees, the test lists them: read each, and if every one is
a correction or a card that now parses, bless them with `UPDATE_GOLDEN=1
cargo test -p vivisurgeons-insight --test golden` and commit the rewritten
files in the same commit as the rule. A moved tree you would not defend is a
rule that reads too much; narrow it rather than bless it. A failed batch is
parked on a branch and `{{branch}}` is reset, so its work is lost.

## Decisions from the supervisor (`{{decisionsPath}}`, the human's file)

These are settled: follow them, and carry out the approved ones that are not
done yet. Report a decision you carried out in report.json's `summary`, with
its commit hash; put a question about one in `escalations`.

{{decisions}}

## Method notes (from the method review of past batches)

{{methodNotes}}

## Queued review findings ({{queueCount}})

{{queue}}

## Failing abilities in {{milestone}} ({{failingCount}}; the first {{failingShown}} below, all of them in `{{failingPath}}`)

{{failing}}

## Handoff from the previous writer (`{{handoffPath}}`)

{{handoff}}
