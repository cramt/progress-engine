# Mana is a gate and a budget; a spell's draw is refused

Mana is modelled twice ([#10](https://github.com/cramt/progress-engine/issues/10)). As a **gate**, `can_cast` is a built-in bipartite matching (Hall's condition over the pips a cost demands). Users cannot assemble it from `produces:` counts, because one Hallowed Fountain satisfies both colours. As a **budget**, `cast` spends a turn's lands on a declared line, and a cast spell leaves the hand. In a file that declares a line, a gate asks what the line left unspent, so there is one pool with one accounting.

What a cast spell then *does* when that is a draw, like Opt's, is **refused by name, not sampled** ([#57](https://github.com/cramt/progress-engine/issues/57)). Every card the walk might draw needs its own checkpoint. On both committed decks that puts the cheapest line over the ceiling by turn five, which is the turn both north stars ask about. The way forward is a non-fixed population ([#18](https://github.com/cramt/progress-engine/issues/18)), not more checkpoints.

See [VISION.md: Mana](../../VISION.md#mana).
