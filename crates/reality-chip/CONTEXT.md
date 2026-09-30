# Reality Chip

The shared core under Ichormoon Gauntlet: what a card is, what a decklist is, what a Scryfall query means, and the exact arithmetic of drawing from a population. It holds the parts that would survive a different product on top.

## Card data

**Card index**:
The dated, line-per-card record of every oracle card, with a header stating its schema, when it was built, and which keywords and oracle tags it carries.
_Avoid_: bulk file, card database

**Card pool**:
Every card in the card index, as opposed to the cards in a deck.
_Avoid_: collection

**Sync**:
Building the card index from Scryfall's bulk data and oracle-tag searches, and dating it.
_Avoid_: download, update

**Oracle tag**:
A Scryfall-curated list of cards that do something (`otag:tapland`, `otag:surveil`), fetched at sync time because it cannot be derived from card text.
_Avoid_: tag (unqualified), label

**Keyword vocabulary**:
Every keyword that appears anywhere in the card pool; a keyword outside it is a typo, not an empty answer.

**Face**:
One side of a card. Statistics, types and costs are read per face, and every card has at least one.

**Query**:
A card selector in Scryfall search syntax, meaning what Scryfall means wherever the key exists. Anything unsupported is an error naming the term, never a silent no-match.
_Avoid_: filter, search, predicate

## Decklists

**Deck**:
A `.deck.toml` ([ADR-0020](../../docs/adr/0020-decks-are-toml-with-typed-categories.md)): cards, each named once and filed in declared categories. Archidekt's text export imports into the same Deck.
_Avoid_: deck file, list

**Collection**:
A `collection.toml` ([ADR-0023](../../docs/adr/0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)): owned cards, each line written as a deck's is, and each in one declared place or unsorted.
_Avoid_: card pool

**Decklist**:
Archidekt's text export, as `gauntlet parse` reads it and `gauntlet import` converts it.

**Printing**:
A card as printed in one set, written `set/number` (`msc/183`). A deck may name a card by printing instead of by name, never both; the card index says which card a printing is.

**Category**:
A label the deck's owner gave a card (`Tutor Package`, `Loam Access`), declared in the deck and queryable as `cat:`. A card may be in any number. It is the owner's, not Scryfall's.
_Avoid_: tag, group

**Category type**:
What a category says about where its cards are, from a fixed tree: in-deck > commander; not-in-deck > sideboard > companion, maybeboard, attractions, sticker-sheet. An untyped category says nothing about it.

**Place**:
Where a card is: the most specific type among its categories, or in-deck when none is typed.

**Commander**:
A card whose place is commander; it starts in the command zone and is not in the library.

**Outside**:
A card whose place is outside the deck: a companion, sideboard, maybeboard or anything else not-in-deck.
_Avoid_: excluded

**Excluded**:
A card whose type can never be in a library (stickers, attractions, planes), dropped when the library is built.
_Avoid_: outside

**Library**:
The deck minus its commanders, outside entries and excluded cards: the population that is drawn from.
_Avoid_: deck (when size matters)

## Drawing

**Population**:
The cards being drawn from, described only as group sizes.

**Group**:
A set of cards that nothing downstream can tell apart, so that only how many of them were drawn matters.
_Avoid_: bucket, class

**Draw**:
Taking cards from the population uniformly at random.

**Removal**:
Taking a known number of cards out of a named group, deterministically, between draws. A tutor is a removal; exiling off the top is a draw.
_Avoid_: tutor (here), fetch (here)

**Sized gap**:
A draw whose size is decided by the path so far rather than fixed before the walk starts. It is the random counterpart of a removal, asked for after every checkpoint right after the removals, and dealt as one more checkpoint; a size of zero costs one composition. A walk with sized gaps has no closed-form width, so it is counted. Decided in [ADR-0017](../../docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md).

**Tail**:
A last draw dealt over a coarsening of the groups: several groups counted as one, because nothing reads them apart. Asked for once every fixed and sized gap is dealt, and again after each tail; its probability is of the count in each bin, and which of a bin's groups holds those cards is not dealt. It is sound for a block whose dealing does not depend on its own cards and that nothing reads before the end, which is the caller's to know. Decided in ADR-0017.

**Checkpoint**:
A point in the sequence of draws where counts are read.

**Path**:
The cumulative count drawn from each group at every checkpoint, with its exact probability.
