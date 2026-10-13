# Chancellor lowers Insight's trees into its own effects, and applies them layer by layer

Chancellor of the Spires derives what is true now (each object's characteristics through CR 613) with ascent, over the game state the Rust state machine holds. [docs/research/rules-engine.md](../research/rules-engine.md) is the plan and the measurements. This settles the seam between them, and revises the plan's point 4, which had Insight's AST going in directly.

- **Insight's trees are lowered into Chancellor's own continuous effects** before they reach ascent. The lowered type is Rust, not a second language, but it is the engine's and not the parser's:
  - ascent relations need `Eq + Hash`, and Insight's AST derives neither;
  - one clause of text is often effects in more than one layer: `Static::Modify { pt, has, .. }`, "get +1/+1 and have flying", is a layer 7c effect and a layer 6 effect (CR 613.6), and the engine wants one fact per layer;
  - **an effect's layer is its variant**, never a field beside it, so a power/toughness change cannot be filed in layer 4. Sublayers 7a–7d are variants too.
- **Counters are state, not rules.** Putting a counter on, removing one, and +1/+1 and -1/-1 annihilating (CR 704.5q) are the state machine's. Ascent only reads them: a counter is a layer 7c modification (CR 613.4c), with its own timestamp (CR 613.7c). A counter in the state is concrete; Insight's `CounterKind::Pt` can say `X` or `*`, and the state cannot.
- **Rust hands ascent the state as input relations:** objects with their printed characteristics, zone, controller, timestamps, counters, and the continuous effects resolved spells and abilities created, each with the set it affects already fixed (CR 611.2c). Ascent recomputes the rest from scratch.
- **The layers are explicit stages in the relations**, as the research's `has_type(s, o, …)` was. Which effects exist depends on layer 6's output, so "has this ability" and "is affected by this effect" read as one recursive question would be negation through a cycle, which ascent cannot stratify. The CR does not ask that question: an effect that started applying in one layer keeps applying to the same set in later layers, even if the ability generating it is removed meanwhile (CR 613.6). Humility with Opalescence is the test that this holds.
- **Tests are rules questions with one right answer,** the CR's own worked examples first, each citing its rule number, against the CR version pinned by hash.

## Considered Options

- **Insight's AST as ascent's facts, deriving `Eq` and `Hash` on it.** Rejected. It is the parser's shape, made to round-trip text, and one node mixes layers. Making the engine read it would either push layer splitting into every rule or bend the parser's tree to the engine.
- **Counters as derived facts, put on and removed by ascent rules.** Rejected. Ascent only adds facts, and a counter coming off is a retraction. A game action is the state machine's.
- **Characteristics as one recursive relation without stages.** Rejected: not stratifiable once abilities can be removed, and not what CR 613.6 says.
