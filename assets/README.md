# Sigils

A symbol for every colour identity, from colorless to five-colour, as SVG. All but `four-color/`
are from [Mana](https://github.com/andrewgioia/mana) by Andrew Gioia (`svg/`, font under SIL
OFL 1.1). The art belongs to Wizards of the Coast.

- `mono/`: the five mana symbols.
- `guilds/`: the ten Ravnica guilds, one per colour pair.
- `clans/`: the five Tarkir clans, the wedges (Abzan WBG, Jeskai URW, Sultai BGU, Mardu RWB, Temur GUR).
- `families/`: the five New Capenna families, the shards (Brokers GWU = Bant, Obscura WUB = Esper,
  Maestros UBR = Grixis, Riveteers BRG = Jund, Cabaretti RGW = Naya). Alara itself never got
  watermarks, so these are the only official symbols for the shard triples. The fan convention
  is the symbols in the Alara charm cycle's art, which have no canonical vector.
- `four-color/`: `sans-white` through `sans-green`, named for the missing colour, because that is
  what players call them; the Nephilim names never caught on. Nothing official or community-made
  exists for these, so each is our own: the missing colour's mana symbol split along the
  top-right diagonal, halves pushed apart, with a bar through the gap. The cut runs that way
  because red's flame curls along the other diagonal and a cut there disappears into it.
- `five-color/`: the planeswalker symbol.
- `colorless/`: the colorless mana symbol, {C}.

Each file's `viewBox` has been reframed from Mana's 32×32 so the sigils read the same size and
centred when drawn at one size: fitted to the drawing, scaled toward equal ink area, and shifted
halfway toward the ink centroid (which is why Azorius's triangle sits a little high in its box).
Every file was measured against the same target, the mean ink area of the twenty guild, clan and
family sigils.

## The tile

`index.html` shows every sigil on its tile, and is the look Meldweb Curator uses. A tile is a
circle with the sigil at 5/6 of its width (100px in 120px) and no ring, on a background in the
identity's colours:

| W | U | B | R | G | colorless |
|---|---|---|---|---|---|
| `#f8f0b0` | `#a9d8f2` | `#888` | `#f4a99a` | `#9fd3a8` | `#cdc6b8` |

One colour or none is a flat fill. Two or more are equal wedges clockwise from the top, in the
order written next to each sigil (`GWU` is green first), with each seam blended over 22°:

```css
/* Selesnya, GW: each colour holds its wedge to 11° short of the seam; the end stops pad past
   0° and 360° with the neighbour so the seam at the top blends like the others */
background: conic-gradient(#f8f0b0 -11deg, #9fd3a8 11deg 169deg, #f8f0b0 191deg 349deg, #9fd3a8 371deg);
```
