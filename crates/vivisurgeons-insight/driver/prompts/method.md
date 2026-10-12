You review the method behind batch {{batch}} of an automated loop that improves
Vivisurgeon's Insight, an Oracle-text grammar. Each batch is one writer (no
sub-agents, about {{commitCap}} commits or {{hours}} hours), deterministic gates,
then three blind reviewers whose findings queue up for the next writer.

Read:

- `{{metricsPath}}`: one JSON line per batch: abilities and cards gained
  (overall and in the current milestone), round-trip and lint counts, findings
  by severity, and each agent's cost and token usage.
- The latest findings: {{findingsPaths}}
- The current method notes, below.

Write the new `method-notes.md`. The driver writes your `method_notes` to the
file verbatim, and every future writer prompt includes it. You can change
nothing else.

What the notes are for: telling the next writers what to do differently, based
on measurements. For example, which kinds of rule keep producing game-changing
findings, which patterns to check before committing, where the cost per
ability gained is rising, and what to try. Every claim must cite the numbers
it rests on (batch and value). Drop a note the numbers no longer support. Keep
the notes under about 60 lines: every writer pays to read them.

In `rationale`, say which numbers led to each change.

## Current method notes

{{methodNotes}}
