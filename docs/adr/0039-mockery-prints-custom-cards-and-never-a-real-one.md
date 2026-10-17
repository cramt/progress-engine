# Mirage Mockery prints custom cards and never a real one

Mirage Mockery renders custom Magic cards to PDF and PNG and lays them out on sheets for printing at home. Phyrexian Metamorph ([ADR-0038](0038-a-proxy-sheet-is-a-list-of-printings-and-a-deck-is-that-list-less-what-it-says.md)) prints real cards, from Scryfall's own images. The two split printing at one line: Metamorph copies real cards, and Mockery makes cards that do not exist.

- **A real card is refused, and the refusal names Metamorph.** On Mockery's own frame, with no art, a real card is a worse proxy than Metamorph's, so it is turned away rather than drawn badly. The card index decides:
  - a name the index knows is refused;
  - so is the same mana cost with the same rules text under another name. Names are compared as `~`, reminder text is dropped, and whitespace and case are ignored;
  - vanilla cards are exempt: empty text matches every vanilla card at its cost, and that is shared design, not a copy.
  - The check runs before every render, and a missing index stops the run rather than skipping the check.
- **The frame is our own.** No Wizards frame art, and no Beleren or Plantin: the fonts are the open ones Typst ships. A custom card should never pass for a printed one. Where a real card has its legal line, ours says "Unofficial fan content · not a Magic card".
- **Every sheet carries the Fan Content Policy's notice**, word for word, in whichever margin has room. Magic has no open licence, as D&D has its SRD under CC-BY, so the [Fan Content Policy](https://company.wizards.com/en/legal/fancontentpolicy) (last updated 2017-11-15) is the only footing. It lets fan content be free to use, with ads or donations, as long as it carries that notice. Wizards can revoke it "for any reason or no reason".
- **The mana symbols are a known risk, and taken.** The policy lists them among the marks not to use; Scryfall, r/custommagic and every custom-card tool use them anyway. Scryfall's SVGs are committed in `mockery-core/symbols/` and compiled in, so a card renders with no network, in the browser too. The repo already carried Wizards' art in the colour-identity sigils in `assets/`, so fetching at run time protected nothing and cost a setup step.
- **The sheet takes Metamorph's parameters but is Mockery's own.** Paper, gap, margin and cut marks mean the same in both, so the two kinds of sheet print alike. But `metamorph-sheet` embeds JPEGs, which suits Scryfall's photographs and not a card drawn in vectors. Measured on four cards: the Typst sheet is 48 KB with vector text, and the same cards as 300 ppi JPEGs through `metamorph-sheet` are 610 KB, with soft text fringed by compression.
- **Insight's reading is a warning, never a gate.** Homebrew may say what Insight cannot read yet, and Insight does not cover all of printed Magic either.

None of this is legal advice. For the record, the policy excludes "creating counterfeit/proxy Magic: The Gathering cards" even when distributed free, and Card Conjurer, a free custom-card maker that showed the notice, got a cease and desist in November 2022 whose first example was importing real card text and art. That is Metamorph's ground, not Mockery's.

## Considered Options

- **Wizards' frames and fonts, as Card Conjurer and Magic Set Editor ship.** Rejected. It is the part of those tools that makes a print look real.
- **Our own glyphs instead of the mana symbols.** Rejected for now. They would read worse, and nothing so far has been enforced over symbols alone.
- **A warning instead of a refusal for real cards.** Rejected. A real card is Metamorph's job, and Metamorph does it better.
- **`metamorph-sheet` for Mockery's sheets.** Rejected on the measurement above: one layout in the family is worth less than print quality.
