---
status: accepted
---

# The site is Progress Engine, in surgical metal over flesh

The site at meldweb.cramt.dk began as Meldweb Curator, the deck editor, and is now the front door to most of the family: Curator's decks and collection, Phyrexian Metamorph's proxies, Mirage Mockery's custom cards and Gitaxian Probe's scanner. A user who meets one member's name on the door has been told the wrong thing.

- **What the site serves says Progress Engine.** That covers the title, the splash, the bar, the login gate, the `mtg` repo's description, and every message that named Curator as the thing speaking. "Their Curator repo" says `mtg` repo instead, since `mtg` is that repo's name.
- **The repo keeps the card names.** `crates/meldweb-curator/`, `meldweb-wasm`, `meldweb.toml`, the worker and the glossary are architecture, and each name still maps to its part (NAMES_FOR_FUTURE.md). The browser keeps its `meldweb.*` storage keys and its `meldweb-scryfall` database. Renaming those would silently forget a user's settings and their copy of Scryfall.
- **The domain and the GitHub App come later, by hand.** The App's install page says Meldweb Curator until it is renamed, and nothing here depends on its name.

## The look

It is blue Phyrexia as a surgeon's tools, not as a screen. It was reached in rounds, and each rejected look says what this one is not:

- Cyan light and glow read as *digital*.
- Screws, bevelled plates, raised buttons and enamel LEDs read as *Fallout*.
- Cyberpunk, Borg and surgical metal were what was asked for, with no buttons and no lights.

So:

- **Flat near-black ribbed like conduit, with steel only at edges and in sheen.** A dialog or the login card is cut at two corners, and its rim is one pixel of polished steel shading into flesh. A rule is a scalpel's edge, bright in the middle and gone at the ends, and staples cross it. The page shades from steel at the top left to flesh at the bottom right.
- **The palette is measured, not guessed.** The blue cards in Scryfall's `arttag:progress-engine`, the tag this family is named from, merge to `#26242A`, `#283250`, `#556998`, `#6A92BB` and `#AEB6CA`, with a flesh `#937B78`. Their blue is slate and periwinkle, so there is no cyan anywhere.
- **Flesh is the one accent.** It marks what is selected or pressed, and the primary action. Mana symbols and colour pips keep their own colours.
- **A control is an engraved label on a steel line.** Uppercase, tracked, with no shape around it. The primary action's line runs from steel into flesh and is stapled.
- **The type is EB Garamond in real small caps** for the wordmark and headings, over the existing sans for everything else. Fontsource's files strip `smcp`, so `scripts/fetch-font.sh` subsets the upstream variable font at a pinned commit and keeps it.
- **The spinner is a suture.** Three steel needles pull flesh thread along the mark's three curves, over their etched trace, and staples land across them. It is one SVG, `src/ui/suture.svg`, inlined into the splash at build time and imported by the route loader, so the two cannot drift apart.
- **The splash is Font of Progress by Aaron J. Riley, credited on screen.** It is kept in `public/splash/` because the site's `require-corp` isolation blocks a CSS background from Scryfall's CDN, which sends no Cross-Origin-Resource-Policy.

## One scheme

The site is dark only. Light mode is dropped rather than left as an unthemed fallback.

## Considered options

- **Rename the crates and files to Progress Engine.** Rejected. The card names are how the repo is navigated, and the stored names in browsers and users' repos are not ours to break.
- **A light mode, as an operating theatre in stainless and porcelain.** It was mocked and is a fair candidate later. It is not built, because a light mode half-done would look worse than none.
- **Hotlinking the art from Scryfall.** Rejected. It is blocked under `require-corp`, and the splash should not wait on Scryfall anyway.
