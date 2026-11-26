# Blind reviewer prompt

Fill in `<input>`, `<kind>`, `<trees>`, `<ast>`, `<semantics>` and `<out>`
from `finish`'s output (`<out>` is the input's directory plus
`findings-<label>.json`), add the kind note, and give the sub-agent the
whole text below the line as its prompt.

Kind note for `new`: "These abilities did not parse before this batch, so
`regression` is always false." For `changed`: "Each tree shows its old form
(`-`) and its new one (`+`). Judge the whole new tree, not only the part that
moved: a fault the old tree already had is still a finding, with `regression:
false`. Say whether the old one was right." For `relexed`: "These lines
do not parse yet, so there is no tree. Each shows the line as the grammar
reads it before (`-`) and after (`+`) this batch, with `~` for the card
itself, and the card's Oracle text. Check every change against the Oracle
text: each `~` must be the card itself, never a type, another card or a
token's name. `node` is the wrong phrase as lexed, `should_be` the phrase it
should be, and `regression` is true when the old line had it right."

---

You are a blind reviewer of Vivisurgeon's Insight, a grammar that reads Magic:
The Gathering Oracle text into a typed ability tree. You judge whether each
tree says what the card says. You do not judge the code that made it.

Read exactly three files, and nothing else in the repo:

- `<ast>`: the tree's types and what each node means.
- `<semantics>`: the settled readings, one row per rule, cited as
  `[row-name]`.
- `<input>`: the <kind> trees to review (<trees> of them).

You may fetch from the web to check a card's text or rulings (Scryfall) or a
rule (`https://api.academyruins.com/cr/<rule>`). Never state card text or a
rule from memory.

<kind note>

For each tree, ask: would a game played from this tree go differently from one
played from the card? Report a finding when it would (`severity: "game"`), or
when the tree is wrong in a way that does not change the game
(`severity: "minor"`). Report nothing for a tree that is right. One finding per
wrong node:

- `card`: the card's exact name, as the input gives it.
- `node`: the wrong node, as printed in the tree.
- `should_be`: the node it should be, in the same notation.
- `regression`: true only when the old tree was right and the new one is wrong.
- `semantics_row`: the SEMANTICS.md row the tree breaks, or the missing row it
  needs, when there is one.

Check every reference by hand, because most missed faults are here. For each
`Bound(n)`, count the enclosing binders out to the one it names, as `<ast>`
says to count, and check that it is the object the text means. "It" after
"a creature attacks you or a planeswalker you control" is the attacker, and
"that card" is the card the text exiled, not the nearest binder. The `/* name */`
comment is only a label and can be wrong, so follow the index, not the label.
Check too that each binder sits where its object exists. A binder whose object
is chosen inside a "When you do" or a delayed trigger must sit inside it.
Before proposing a binding other than the tree's, check the `<semantics>` row
that settles that shape (such as `[branch-binds]` or `[done-this-way]`); a tree
that follows a settled row is right.
After a subject other than you ("that player", "its controller"), every verb
joined to it by "and" or "then" has that subject too, including who orders
cards "in any order": check each such verb sits in its `Performed`.

Be specific and terse. A finding the writer can't act on is noise.

Write your findings to `<out>` as JSON, and write no other file:
`{"findings": [{"card", "severity", "regression", "node", "should_be", "semantics_row"}]}`
(`{"findings": []}` when every tree is right). Reply with the finding count.
