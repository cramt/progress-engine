# Scenarios

Rules interactions the engine should eventually get right, each a test
waiting to be written. They are distilled from two YouTube channels that
explain Magic's rules with concrete boards: Keeping It Casual's shorts
(`@keepingitcasualmtg`) and This Is A Commander Channel's *Tricky Rules &
Card Interactions* (`@thisisacommanderchannel`). The replay tests the engine
against what players did; these test it against what the rules say about
corners the logs rarely reach.

A scenario keeps what a video claims, never its transcript. `fetch.sh`
caches the auto-captions in `~/.cache/chancellor/yt/`, outside the repo.

## A video is not the authority

The Comprehensive Rules are. Every scenario's expected outcome is checked
against the pinned CR and the cards' Oracle text, and cites the rules it
rests on as "CR nnn.n" so `cr check` resolves them. Where the video says
otherwise, the scenario says what the rules say, and notes the
disagreement. Auto-captions garble card names ("calling ritual" is Culling
Ritual): every name is checked against Scryfall before it is written.

## Files

One file per theme (`layers.md`, `replacement.md`, `stack-and-priority.md`,
`copies.md`, `combat.md`, `costs.md`, `zones.md`, `triggers.md`, …), the
scenarios in it numbered in order of addition. A video that isn't about
the rules (a dice box, how to pronounce a card) gives none.

## A scenario

```markdown
### N. A short claim about the outcome, as the test's name would be

**Cards:** [Culling Ritual], [Blood Artist], [Bloodchief Ascension]
**Board:** who controls what, in which zone, life totals and counters where
they matter, and whose turn and step it is.
**Does:** the actions in order, who has priority, what is on the stack.
**Then:** the outcome, stated so a test can assert it: life totals, zones,
what triggered and in what order.
**Because:** CR 603.3b, CR 101.4; one or two sentences of why.
**Source:** [Video title](https://www.youtube.com/watch?v=ID)
**Engine:** what Chancellor would need first (a refused node, a missing
mechanic), or "runs today" if a test could be written now.
```

`**Disagrees:**` goes after `**Because:**` when the video's answer differs
from the rules', saying how. `**Unsure:**` goes there instead when the
rules don't settle it, saying what is open, rather than guessing. An
Oracle ruling that settles a point is cited in `**Because:**` by card and
date.

A scenario is one claim, so it is one test: a video with three cases is
three scenarios, not one with a list.
