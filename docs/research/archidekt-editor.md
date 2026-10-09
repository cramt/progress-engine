# Archidekt's deck editor, taken apart

The plan for the git-backed deck editor is to copy Archidekt's editor first
and change it second, because hand-designed agent frontends have come out
unusable before. This file is what that editor actually does. It was written
from a walkthrough on 2026-09-27: `decks/lantern.txt` imported unchanged into
Archidekt's sandbox (`archidekt.com/sandbox`, a full editor that needs no
account), and the owner's public view of the same deck
(`archidekt.com/decks/25088186`). Screenshots were taken but are not
committed, because this repo is public and they are Archidekt's UI.

## The file format already round-trips

Archidekt imported `lantern.txt` with its `//` comments stripped and reported
"All cards listed have been found", `[Commander{top}]` and the printings
included. The dialect its import box documents:

```
2 Sensei's Divining Top
2x Sol Ring
1x Path to Exile (cmm) [Removal] ^To Remove,#FF0000^
# Sideboard
1 Wrath of God
```

`^Label,#RRGGBB^` is a **colour tag**, a per-card label separate from
categories. `chip-decklist` has to accept it before the editor can write it.
`# Heading` groups the following lines into a category, an alternative to
per-line brackets.

## Page layout, top to bottom

1. **Header**: deck name, format, legality, estimated bracket, size, estimated
   cost, salt sum, deck tags. Buttons: Import cards, More, Playtester.
2. **Toolbar**, sticky on scroll:
   - *Card search* button (opens the search overlay) and a *Quick add* box
     (`Ctrl+'`), which autocompletes names and shows printing and price per
     suggestion. Its gear sets options (presumably the target category).
   - *View as*: Stacks, Grid, Text list, Table (with a gear for per-view
     options). *Group by*: Categories, Type, and others. *Sort by*: Alphabet,
     and others.
   - Price sources (not wanted: Curator prices from Scryfall, in its own
     Cost view, `?view=cost`).
   - *Local filter*: filters the deck in place.
3. **Layout presets**: four thumbnails that set View as and Group by together.
   They dismiss once chosen.
4. **Left rail**, sticky: saved state, undo, redo, playtest, settings, more.
5. **The deck**, as masonry columns, one column per category.
6. An optional **pinned side panel** (layout 4): deck info, colour cost against
   colour production per colour, mana curve with the average and total MV, and
   quantity per category. Its "Condensed view" dropdown switches panels.

## The four layouts

| Preset | View | Group | What it looks like |
|---|---|---|---|
| 1 | Stacks | Categories | The default and the owner's choice. Each card shows only its name bar, overlapped, and the last card in a category shows in full. Hovering a card fans the stack open at that card and pushes the rest down. |
| 2 | Text | Type | Two columns of `qty name  mana-cost  ⋮` rows, plus a sticky right panel with the hovered card's image, printing picker, foil toggle and categories. |
| 3 | Text | Categories | Three dense text columns. Hovering a row shows a floating card image. |
| 4 | Grid | Categories | Full card images in a grid, with the stats panel pinned on the right. |

Every category header shows `Name`, `Qty: n`, `Price: x` and a `...` menu.
The Commander category carries a crown icon and always comes first.

## The card is one component with one menu

The card in a stack, in the grid, in a text row and in search results is the
same object with the same `...` context menu. Only which entries are enabled
changes: in search results "Decrease quantity" and "Add as new card" are
greyed out. Copy this. It is why the editor feels consistent.

When a card is fully visible in edit mode, it shows `+`, `−` and `...` down
its right edge. Clicking the card opens the details modal.

### The context menu, with its hotkeys

| Entry | Key |
|---|---|
| Open details | click |
| Increase quantity | `+` |
| Decrease quantity | `-` |
| Add as new card (a second copy with its own printing and categories) | |
| Pin card | `W` |
| Switch card printing | `P` |
| Set as commander | |
| Move to category ▸ Automatic `A`, Maybeboard `M`, Sideboard `S`, Create new category, then every existing category | |
| Change color tag ▸ | |
| Remove card | `R` |
| Multi-select | `Ctrl+Click` |
| Compare card with… | |
| Card extras ▸ View card page, Copy card name `C`, EDHREC, Scryfall, TCGPlayer, Card Kingdom, View other decks | |

The hotkeys act on the hovered card. The practical loop is: hover, press a
key, move to the next card, with no clicks and no modals.

### Card details modal

- Big card image on the left. Prev/next buttons at the bottom step through
  the deck in display order without closing the modal. This is how you walk a
  whole deck fixing printings.
- Quantity field with `−`/`+`.
- Printing dropdown, an **All printings** button, and a Foil/Nonfoil toggle.
- **Categories**: a list, with the star marking the premier (`{top}`)
  category. Each has an ✕, and there is *Add Category*. A card can be in
  several categories, which the text format already allows.
- Quick category buttons: Ramp, Sideboard, Maybeboard, Oracle tags (suggests
  categories from Scryfall's oracle tags).
- Other options: colour tag, custom MV override, *Set Deck Image*.
- Tabs: Card options, Oracle tags, In decks, Collection records, more.

### All printings

A grid of every printing, with a set-name filter, an order-by option (edition
date), an image size toggle and a price under each card. The current printing
is outlined and labelled "Selected printing". It keeps the prev/next deck
navigation.

## Drag and drop

Dragging a card turns every category column into a large drop target with a
`+` and the category name, captioned "(CTRL to add secondary)". A plain drop
moves the card. A Ctrl drop adds the category and keeps the old one. A strip
across the top adds drop zones for **Auto, New Category, Maybeboard,
Sideboard, Pinned Cards**. Tested: Sol Ring from Artifact Count to Draw
updated both headers' counts at once (8→7, 4→5).

## Category menu

Collapse, View cards in grid (per category, overriding the view), Edit
category (rename, and whether the category counts toward the deck), Select
all, Hide maybeboard(s), Copy card names, Copy card names with quantity, Sell
this stack.

## Card search overlay

- Opens full-screen over the deck. **Lock** docks it as a side panel so you
  can search and drag at the same time.
- Two modes: *Archidekt search* (a form with Advanced Options and Filter &
  Sort) and **Syntax search** (Scryfall syntax, with a "Syntax guide" link).
- *Apply smart filters*, on by default, adds the deck's format and colour
  identity to every query. `t:artifact mv<=2 id<=urg o:draw` gave 177 results.
- Multiple search tabs (`+`, with a count badge) and a History of past
  searches.
- Results use the same card component: `+` adds a copy, `...` opens the full
  menu, and dragging a result drops it into a category.

## What to copy first

In order. Each item is usable without the ones after it.

1. The Stacks + Categories view with hover-to-fan, category headers with
   counts, and the sticky toolbar. Reading the deck is the thing done most.
2. The single card component with the context menu and **hover hotkeys**
   (`+ - P R A M S`).
3. The details modal with prev/next, quantity, categories with the premier
   star, and the printing dropdown.
4. Drag between categories, with the Ctrl-to-add-secondary rule and the top
   drop strip.
5. Quick add with autocomplete.
6. The card search overlay with syntax search and smart filters, and Lock.
   `chip-scryfall` already parses the query syntax.
7. The All printings grid.
8. The Text/Categories and Grid views, and the stats panel. The stats panel is
   where Gauntlet's numbers would go, next to the curve.

## What not to copy

Price sources (Curator has its own Cost view, priced from Scryfall), salt,
"Sell this stack", views, likes, the Patreon
banners, the TCGPlayer and Card Kingdom links, and collection records. The
sandbox's save model goes too: saving is a commit.
