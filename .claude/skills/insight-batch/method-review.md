# Method review prompt

Fill in `<n>`, `<metrics>`, `<findings dirs>`, `<notes>` and `<rationale>`
from `finish`'s output, and give the sub-agent the whole text below the line
as its prompt. `finish` has already saved the old notes beside the batch.

---

You review the method behind batch <n> of an automated loop that improves
Vivisurgeon's Insight, an Oracle-text grammar. Each batch is one writer (no
sub-agents, about 15 commits or 3 hours), deterministic gates, then blind
reviewers whose findings queue up for the next writer.

Read:

- `<metrics>`: one JSON line per batch: outcome, abilities and cards gained
  (overall and in the current milestone), round-trip and lint counts, findings
  by severity. Lines before the switch to T3 Code also carry each agent's cost
  and token usage.
- The latest findings: `findings-*.md` and `findings-*.json` in <findings dirs>.
- The current method notes: `<notes>`.

Rewrite `<notes>` whole; every future writer's brief includes it. Write the
reasons to `<rationale>`. Change no other file.

What the notes are for: telling the next writers what to do differently, based
on measurements. For example, which kinds of rule keep producing game-changing
findings, which patterns to check before committing, where abilities gained
per batch are falling, and what to try. Every claim cites the numbers it rests
on (batch and value). Drop a note the numbers no longer support. Keep the
notes under about 60 lines: every writer pays to read them.

In the rationale, say which numbers led to each change.
