# A land whose tapped-ness the pilot chooses is assumed tapped, and named

A shockland's tapped-ness is a choice, and nothing in the card data settles it. The run assumes the pessimistic half and lists every card it assumed it about. A number a deck can beat is better than one it cannot reach. The assumption is meant to become declarable.

What cannot be known is refused rather than guessed in whichever direction flatters. That covers what a fetched land taps for (`otag:fetchland` holds both Scalding Tarn and Terramorphic Expanse) and a land put onto the battlefield by a spell (no tag separates Rampant Growth from Nature's Lore). It also covers castability on an index missing `produces` or either tapland tag.

Amended by #82: a fetchland *played as a land* — no fetch effect declared — is not unknowable. Its oracle text says what it searches for and whether the land arrives tapped, and the deck says which such lands exist, so it is read as the untapped lands it can find, and named with its one assumption: that one of them is still in the library. A mana question beside a declared fetch *effect* is still refused.

See [VISION.md: Decided](../../VISION.md#decided).

Amended: the assumption is declarable. `[assume] untapped = [...]` names the conditional taplands the pilot plays untapped — a shockland whose 2 life is paid, a Battlebond land in a game with two or more opponents — and every run that read one so prints it beside `assumed_tapped`. Undeclared, the pessimistic half still stands. A land tagged `otag:tapland` has no condition to settle and is refused by name. The condition is never evaluated: the opponent count is a fact about the game, not the deck (ADR-0005), and a checkland's basic-type condition would need land types in the grouping.

Superseded in part by [ADR-0025](0025-a-land-a-spell-puts-down-is-tapped-and-pays-from-the-next-turn.md): a land put onto the battlefield by a spell or an activation is no longer refused. It takes the pessimistic half, as a conditional tapland does: read as entering tapped, and paying from the next turn.
