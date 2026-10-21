// One custom card, and a sheet of them. The frame is Mirage Mockery's own,
// not a copy of Wizards': a print should never pass for a real card.
//
// The data arrives as /job.json, written by render.rs, so no card text is
// ever spliced into Typst source.

#let job = json("/job.json")

#let palette = (
  white: rgb("#e9e2c8"),
  blue: rgb("#8fb8d8"),
  black: rgb("#9a9192"),
  red: rgb("#e0997a"),
  green: rgb("#93b98c"),
  gold: rgb("#dcc27a"),
  artifact: rgb("#b4bcc2"),
  land: rgb("#c4ae94"),
  colorless: rgb("#cfcbc6"),
)

// The colour indicator's own colours, stronger than the frame's so a dot of
// them reads at 3 mm.
#let ink = (
  white: rgb("#f4efd9"),
  blue: rgb("#2f73b0"),
  black: rgb("#2b2526"),
  red: rgb("#c8402e"),
  green: rgb("#2f7a45"),
)

#let paper = rgb("#f5f0e6")

#let card-width = 63mm
#let card-height = 88mm

// Where a face's parts stand, from the card's top edge, measured on printed
// M15-frame cards at 745 by 1040 px (Lotus Cobra, Lightning Helix, Llanowar
// Elves, Counterspell, Wurmcoil Engine, Kolaghan's Command, Fabled Passage
// and Emeria's Call agree to a pixel or two). A bar's top and height are its
// fill's; its rim stands round that. The frame stops just under the text box,
// and the black band under it, 7% of the card, holds the footer, where a
// printed card's collector line is.
#let border-top = 2.3mm
#let border-side = 2.6mm
#let layout-of(kind) = if kind == "saga" or kind == "class" {
  // A Saga's or a Class's columns run down to the type line at the foot
  // (Fable of the Mirror-Breaker, Wizard Class).
  (name-top: 4.06mm, name-h: 4.4mm, art-top: 9.4mm, art-bottom: 73.6mm,
    type-top: 74.3mm, type-h: 4.55mm, foot: 80.8mm)
} else if kind == "planeswalker" {
  // A planeswalker's name bar stands higher and shallower than other
  // cards', and its window starts just under it (Teferi, Hero of Dominaria;
  // Chandra, Torch of Defiance).
  (name-top: 3.4mm, name-h: 4.05mm, art-top: 8.7mm, type-h: 4.55mm,
    box-bottom: 80.4mm, foot: 81.15mm)
} else {
  // The art runs from just under the name bar's rim to under the type
  // line's, which stands over its foot.
  (name-top: 4.06mm, name-h: 4.4mm, art-top: 9.55mm, art-bottom: 49.5mm,
    type-top: 49.6mm, type-h: 4.55mm, box-top: 55.1mm, box-bottom: 81.4mm, foot: 81.75mm)
}

#let sym(path, size) = box(baseline: 14%, image(path, height: size))

#let paragraph(runs) = {
  for r in runs {
    // An ability or flavor word opening the paragraph is italic, as is
    // reminder text (CR 207.2a, 207.2c–d).
    if "symbol" in r { sym(r.symbol, 0.9em) } else if r.reminder or r.at("word", default: false) { emph(r.text) } else { r.text }
  }
}

// Colours split hard, side by side, as a dual land's text box or a
// two-colour indicator is.
#let split(colors, angle: 0deg) = if colors.len() == 1 { colors.first() } else {
  let n = colors.len()
  let stops = ()
  for (i, c) in colors.enumerate() {
    stops.push((c, i / n * 100%))
    stops.push((c, (i + 1) / n * 100%))
  }
  gradient.linear(..stops, angle: angle)
}

// The frame's paints. A card's colour is its frame; a land's frame is a
// land's, tinted by the colours it is or makes, and two of them split down
// the middle as printed dual lands are.
// The name and type bars' rim, in the card's colour as printed bars' is: a
// gold card's in its two colours, left to right, and a land's in the colours
// it is or makes.
#let rim-paint(frame) = {
  let rims(names) = if names.len() == 1 { ink.at(names.first()) } else if names.len() == 2 {
    gradient.linear(..names.map(k => ink.at(k)))
  } else { palette.gold.darken(30%) }
  if frame.base in ink { ink.at(frame.base) }
  else if frame.base == "gold" { rims(frame.at("colors", default: ())) }
  else if frame.base == "land" { if frame.tint.len() == 0 { rgb("#d9c47c") } else { rims(frame.tint) } }
  else if frame.base == "artifact" { rgb("#e3e6ec") }
  else { luma(185) }
}

#let paints(frame) = (rim: rim-paint(frame)) + {
  let base = palette.at(frame.base)
  let tint = frame.tint.map(t => palette.at(t))
  if frame.base == "gold" or (frame.base == "land" and tint.len() == 0) {
    // A gold card's bars, and a colourless land's, are filled in their
    // frame's own tone rather than paled, as printed ones are.
    (
      body: gradient.linear(base.lighten(15%), base, angle: 90deg),
      bar: if frame.base == "gold" { rgb("#c9b27a") } else { rgb("#d8c483") },
      box: paper,
      accent: base,
      foot: base,
    )
  } else if frame.base != "land" or tint.len() == 0 {
    (
      body: gradient.linear(base.lighten(15%), base, angle: 90deg),
      bar: base.lighten(55%),
      box: paper,
      accent: base,
      foot: base,
    )
  } else if tint.len() == 1 {
    let c = tint.first()
    let mixed = color.mix((base, 25%), (c, 75%))
    (
      body: gradient.linear(mixed.lighten(15%), mixed, angle: 90deg),
      bar: c.lighten(55%),
      box: c.lighten(68%),
      accent: c,
      foot: mixed,
    )
  } else if tint.len() == 2 {
    // Blended across rather than cut, as a dual land's text box is.
    let mixed = tint.map(c => color.mix((base, 15%), (c, 85%)).darken(8%))
    (
      body: gradient.linear(..mixed),
      bar: base.lighten(55%),
      box: gradient.linear(..tint.map(c => c.lighten(50%))),
      accent: base,
      foot: mixed.last(),
    )
  } else {
    let mixed = color.mix((base, 25%), (palette.gold, 75%))
    (
      body: gradient.linear(mixed.lighten(15%), mixed, angle: 90deg),
      bar: palette.gold.lighten(55%),
      box: palette.gold.lighten(68%),
      accent: palette.gold,
      foot: mixed,
    )
  }
}

// A line that has one line to itself, the name or the type line, shrinks to
// fit it as printed cards do, and a card it still overflows is refused
// rather than printed with its frame pushed out of place.
#let fit-line(body, start, floor, what) = layout(size => {
  let set-at(sz) = text(size: sz, weight: "bold", body)
  let sz = start
  while sz > floor and measure(set-at(sz)).width > size.width { sz -= 0.25pt }
  if measure(set-at(sz)).width > size.width {
    panic(what + " does not fit on one line even at " + repr(floor))
  }
  set-at(sz)
})

// A paint made `amount` see-through: a colour as Typst does it, and a
// gradient (a land's box, tinted by every colour it makes) stop by stop,
// which Typst's gradients have no method for.
#let see-through(paint, amount) = if type(paint) == gradient {
  gradient.linear(..paint.stops().map(((c, at)) => (c.transparentize(amount), at)), angle: paint.angle())
} else { paint.transparentize(amount) }

// Text set to fill a box: printed cards shrink their type to fit rather than
// spill, so this does too, to the quarter point, from 9.5pt, where
// Libertinus's x-height is a printed card's largest rules text's, 1.48mm. `body(sz, spare)` is the
// text at a size, handed the height left over so sections can share it out.
// The largest quarter point that fits is found by halving, not by stepping
// down: every try lays the text out, and a Saga's sections each twice.
// The floor is 5pt as printed: a meld pair's melded card is drawn smaller and
// scaled up, so its face carries a lower `floor` by that scale, and Urza,
// Planeswalker's six abilities fit its box at the size a card's would.
#let floor-of(c) = c.at("floor", default: 5pt)
#let fit-box(body, what: "the rules text", max: 9.5pt, floor: 5pt) = layout(size => {
  let height(sz) = measure(block(width: size.width, body(sz, 0pt))).height
  let fits(q) = height(floor + q * 0.25pt) <= size.height
  // Quarter points above the floor: 0 is 5pt, 18 is 9.5pt.
  let (lo, hi) = (0, calc.floor((max - floor) / 0.25pt))
  if not fits(lo) { panic(what + " does not fit its box even at " + repr(floor)) }
  if fits(hi) { lo = hi }
  while hi - lo > 1 {
    let mid = calc.quo(lo + hi, 2)
    if fits(mid) { lo = mid } else { hi = mid }
  }
  let sz = floor + lo * 0.25pt
  body(sz, size.height - height(sz))
})

// Lines and paragraphs spaced as printed: measured on Lotus Cobra, Lightning
// Helix, Kolaghan's Command, Questing Beast and Teferi, a printed card's
// lines stand 2.15 x-heights apart at any size, and its paragraphs 0.5-1mm
// further. Libertinus's x-height is 0.44em.
#let text-style(sz, body) = {
  set text(size: sz)
  set block(spacing: 0.9mm)
  set par(leading: 0.3em, spacing: 0.3em + 0.7mm, justify: false)
  body
}

#let paragraphs(lines) = for p in lines { par(paragraph(p)) }

#let flavor-text(c) = text(style: "italic", c.flavor)

// The rule between rules text and flavor text: a hairline fading out at both
// ends, 0.63em under the rules text's last baseline and 0.48em over the
// flavor text's capitals, as Lotus Cobra's and Lightning Helix's are.
#let divider() = block(above: 0.63em, below: 0.48em, width: 100%, height: 0.4pt,
  fill: gradient.linear(luma(120).transparentize(100%), luma(120), luma(120), luma(120).transparentize(100%)))

// A loyalty cost's badge (CR 606.4), Mockery's own: a dark plate whose top
// rises to a point for a cost that adds counters, whose foot drops to one for
// a cost that removes them, and flat for neither.
#let loyalty-badge(cost, sign) = {
  let (w, h, tip) = (6mm, 3.7mm, 0.9mm)
  let pts = if sign == "plus" {
    ((0mm, tip), (w / 2, 0mm), (w, tip), (w, h), (0mm, h))
  } else if sign == "minus" {
    ((0mm, 0mm), (w, 0mm), (w, h - tip), (w / 2, h), (0mm, h - tip))
  } else {
    ((0.6mm, 0mm), (w - 0.6mm, 0mm), (w, h / 2), (w - 0.6mm, h), (0.6mm, h), (0mm, h / 2))
  }
  box(width: w, height: h, {
    place(polygon(fill: luma(28), stroke: 0.9pt + luma(215), ..pts))
    place(center + horizon, dy: if sign == "plus" { 0.3mm } else if sign == "minus" { -0.3mm } else { 0mm },
      text(size: 7pt, weight: "bold", fill: white, cost))
  })
}

// The loyalty shield, rim and all, and how far its horns stand above the text
// box's foot: measured on Teferi, Chandra, Minsc & Boo, Karn, Liliana and
// Ugin, which agree to a pixel, 107 by 67 px of 745 by 1040 with the horns
// 35 px over the box's foot.
#let walker-shield = (8.96mm, 5.67mm)
#let shield-rise = 2.9mm

// Power and toughness on a raised plate in the bars' colours, its corners
// cut, as printed modern frames' plate is. Measured on Lotus Cobra and
// Wurmcoil Engine: 16% of the card's width by 5.6% of its height, rim and
// all, wider for a longer power and toughness.
#let pt-plate(pt, ps) = context {
  let label = text(weight: "bold", size: 10pt, pt)
  let (w, h) = (calc.max(10.15mm, measure(label).width + 3.4mm), 4.9mm)
  let cut = 0.9mm
  let shape(g) = (
    (cut - g * 0.4, -g), (w - cut + g * 0.4, -g), (w + g, cut - g * 0.4), (w + g, h - cut + g * 0.4),
    (w - cut + g * 0.4, h + g), (cut - g * 0.4, h + g), (-g, h - cut + g * 0.4), (-g, cut - g * 0.4))
  let rim = 0.5mm
  box(width: w, height: h, {
    place(dy: 0.3mm, polygon(fill: black.transparentize(65%), stroke: none, ..shape(0mm)))
    place(polygon(fill: ps.rim, stroke: 0.8pt + luma(25), ..shape(0mm)))
    place(polygon(fill: ps.bar.darken(28%), stroke: none, ..shape(-rim)))
    place(dx: 0.2mm, dy: -0.22mm, polygon(stroke: none,
      fill: gradient.linear(ps.bar.lighten(20%), ps.bar, ps.bar.darken(12%), angle: 90deg),
      ..shape(-rim - 0.18mm)))
    place(polygon(fill: none, stroke: 0.45pt + luma(25), ..shape(-rim)))
    place(center + horizon, dy: 0.1mm, label)
  })
}

// The corner: power and toughness or defense on a bar-coloured plate, and a
// planeswalker's loyalty on a dark shield, pointed at the foot, so loyalty
// never reads as a power and toughness. The shield is Mockery's drawing of
// printed walkers' shape: its top low at the corners, rising to a sharp horn
// either side of the middle and scooped round between the horns down to the
// corners' height, its sides dented in as they fall to where its foot runs
// to a point, under a thin light rim. Measured inside the rim on the six
// walkers above, as fractions of that 93 by 54 px: the corners 19% down, the
// horns 18% either side of the middle, the sides' feet 7% in and 76% down.
#let corner-box(c, ps) = if c.corner_kind == "loyalty" {
  let (w, h) = walker-shield
  let rim = 0.36mm
  let (iw, ih) = (w - 2 * rim, h - 2 * rim)
  let mid = iw / 2
  let (corner, horn, side-in, side-foot) = (0.19 * ih, 0.18 * iw, 0.07 * iw, 0.76 * ih)
  let fall = side-foot - corner
  let outline(..style) = place(dx: rim, dy: rim, curve(..style,
    curve.move((0mm, corner)),
    curve.line((mid - horn, 0mm)),
    // Steeply off the horn, rounding into the scoop's foot.
    curve.cubic((mid - horn + 0.05 * iw, 0.55 * corner), (mid - 0.08 * iw, corner), (mid, corner)),
    curve.cubic((mid + 0.08 * iw, corner), (mid + horn - 0.05 * iw, 0.55 * corner), (mid + horn, 0mm)),
    curve.line((iw, corner)),
    curve.cubic((iw - 0.8 * side-in, corner + 0.25 * fall), (iw - side-in, side-foot - 0.45 * fall), (iw - side-in, side-foot)),
    curve.line((mid, ih)),
    curve.line((side-in, side-foot)),
    curve.cubic((side-in, side-foot - 0.45 * fall), (0.8 * side-in, corner + 0.25 * fall), (0mm, corner)),
    curve.close(mode: "straight")))
  box(width: w, height: h, {
    // The rim is the outline's stroke, edged grey outside, and the dark fill
    // over its inner half.
    outline(stroke: (paint: luma(115), thickness: 2 * rim + 0.5pt, join: "miter", miter-limit: 3))
    outline(stroke: (paint: luma(222), thickness: 2 * rim, join: "miter", miter-limit: 3))
    outline(fill: luma(20), stroke: none)
    place(center + horizon, dy: 0.3mm, text(weight: "bold", size: 10pt, fill: white, c.corner))
  })
} else if c.corner_kind == "pt" {
  pt-plate(c.corner, ps)
} else {
  box(fill: ps.bar, stroke: 0.4pt + black, radius: 1mm, inset: (x: 2mm, y: 1mm),
    text(weight: "bold", size: 10pt, c.corner))
}

// Split corner to corner, the first colour top left, as printed indicators are.
#let indicator(colors) = circle(radius: 1.35mm, stroke: 0.4pt + black,
  fill: split(colors.map(k => ink.at(k)), angle: 45deg))

// A Saga's chapter symbol: a numeral on a hexagon (CR 714.2a).
#let chapter-mark(numeral, ps) = box(width: 5.2mm, height: 5.6mm, {
  // Points up and down, flat sides left and right, as the chapter symbol is.
  let r = 2.6mm
  let hex = range(6).map(i => (r * calc.cos(30deg) + r * calc.cos(90deg + i * 60deg), r + r * calc.sin(90deg + i * 60deg)))
  place(center + horizon, polygon(fill: ps.accent.lighten(25%), stroke: 0.4pt + black, ..hex))
  place(center + horizon, text(size: if numeral.len() > 3 { 4.6pt } else { 6.5pt }, weight: "bold", numeral))
})

// A station symbol: the threshold on a dark disc (CR 721.2).
#let threshold-mark(at, ps) = box(width: 5.6mm, height: 5.6mm, {
  place(center + horizon, circle(radius: 2.8mm, fill: luma(30), stroke: 0.4pt + ps.bar))
  place(center + horizon, text(size: if at.len() > 3 { 4.6pt } else { 6pt }, weight: "bold", fill: white, at))
})

// The standard text box: plain paragraphs, or a station card's striations,
// each threshold a band down to the next with its mark on the box's left
// edge, and the last band holding the power and toughness (CR 721.2b).
#let standard-text(c, ps, max: 9.5pt, height: 1fr, fill: auto, stroke: 0.4pt + black, top: 1.8mm, clear: auto, pad-right: auto, centre-lone: true) = {
  let r = c.rules
  // Printed rules text runs to within 1mm of the box's sides.
  let pad = 1.2mm
  let striated = r.kind == "station" and r.thresholds.len() > 0
  let corner-in-band = striated and c.corner != none
  // Clear of what stands over the box's foot: the hint bar, the power and
  // toughness plate's upper part, or a back's reminder.
  let bottom = if clear != auto { clear } else if c.hint != none { 4.2mm } else if c.corner != none and not corner-in-band { 3.6mm } else if c.back_pt != none { 3.2mm } else { 1.8mm }
  block(width: 100%, height: height, fill: if fill == auto { ps.box } else { fill }, stroke: stroke, clip: false,
    inset: (left: pad, right: if pad-right == auto { pad } else { pad-right }, top: top, bottom: bottom),
    fit-box(max: max, floor: floor-of(c), (sz, spare) => text-style(sz, {
      // Text that leaves room is centred in the box, as printed cards set a
      // short text; a striated box fills it instead.
      if not striated { v(spare / 2) }
      // A lone line of rules text is centred across the box, as
      // Counterspell's and Wear // Tear's are.
      // A half box's, as Seething Song's, is set from the left.
      let lone = centre-lone and r.kind == "plain" and r.lines.len() == 1 and c.flavor == none
      if lone { layout(room => {
        let line = paragraph(r.lines.first())
        if measure(line).width <= room.width { align(center, par(line)) } else { par(line) }
      }) } else if r.kind == "plain" { paragraphs(r.lines) } else {
        paragraphs(r.before)
        let last = r.thresholds.len() - 1
        for (i, t) in r.thresholds.enumerate() {
          let tail = i == last
          let room = if tail and corner-in-band { 11mm } else { 0mm }
          let band(h) = block(width: 100%, height: h, breakable: false,
            // Each striation a step darker than the one above it.
            fill: ps.bar.darken(6% * i), stroke: (top: 0.3pt + black),
            outset: (x: pad, bottom: if tail and c.flavor == none { 1.8mm } else { 0mm }),
            above: 1.6mm, below: 0mm, inset: (left: 4.4mm, right: room, y: 1mm), {
              place(left + horizon, dx: -4.4mm - pad - 0.4mm, threshold-mark(t.at, ps))
              paragraphs(t.text)
              if tail and corner-in-band {
                place(right + horizon, dx: room + pad + 0.6mm, corner-box(c, ps))
              }
            })
          if tail and c.flavor == none {
            // The last striation runs to the foot of the box.
            layout(size => band(measure(band(auto), width: size.width).height + spare))
          } else { band(auto) }
        }
      }
      if c.flavor != none {
        if r.kind != "plain" or r.lines.len() > 0 { divider() }
        flavor-text(c)
      }
    })))
}

#let art-box(c, ps, height: 100%) = block(width: 100%, height: height, stroke: 0.4pt + black, clip: true,
  if c.art != none {
    image(c.art, width: 100%, height: 100%, fit: "cover")
  } else {
    let k = if c.frame.tint.len() > 0 { palette.at(c.frame.tint.first()) } else { palette.at(c.frame.base) }
    rect(width: 100%, height: 100%, stroke: none,
      fill: gradient.linear(k.darken(35%), k.lighten(10%), angle: 60deg))
  })

// A transforming front's grey reminder of its back's power and toughness
// (CR 712.2c).
#let back-pt(c) = text(size: 6.5pt, weight: "bold", fill: luma(105), c.back_pt)

// A Saga's or a Class's text column: a first section across the column, then
// sections that share the height left over, as printed Sagas and Classes are
// striated (CR 714.1, 716.1).
#let column-text(c, ps) = block(width: 100%, height: 100%, fill: ps.box, stroke: 0.4pt + black,
  fit-box(floor: floor-of(c), (sz, spare) => text-style(sz, {
    let r = c.rules
    let pad = (x: 1.6mm, y: 1.3mm)
    let sections = if r.kind == "saga" { r.chapters } else { r.levels }
    let share = spare / calc.max(sections.len(), 1)
    let fill-to(body) = layout(size => {
      let h = measure(block(width: size.width, body(auto))).height
      body(h + share)
    })
    set block(spacing: 0pt)
    // A Class's reminder is set smaller and ruled off from its first level's
    // abilities, as printed Classes set it.
    let reminder(p) = p.all(run => "symbol" in run or run.reminder)
    if r.before.len() > 0 {
      block(width: 100%, inset: pad, for (i, p) in r.before.enumerate() {
        if r.kind == "class" and reminder(p) {
          par(text(size: 0.85em, paragraph(p)))
          if i + 1 < r.before.len() { block(above: 0.9mm, below: 1mm, line(length: 100%, stroke: 0.3pt)) }
        } else { par(paragraph(p)) }
      })
    }
    if r.kind == "saga" {
      // The chapter symbols straddle the column's left edge, as printed
      // Sagas' do, so the text runs from just past them. A transforming
      // Saga's reminder of its back's power and toughness sits in the last
      // chapter's corner under them, as Fable of the Mirror-Breaker's does
      // (CR 712.2c), the section tall enough to hold both.
      let back-reminder = c.back_pt != none and c.corner == none
      let mark-w = 5.2mm
      let into = 0.4mm
      let text-at = mark-w / 2 + into + 0.9mm
      let last = sections.len() - 1
      for (i, ch) in sections.enumerate() {
        let tail = i == last and back-reminder
        let marks = stack(spacing: 0.6mm, ..ch.numerals.map(n => chapter-mark(n, ps)))
        let section(h) = block(width: 100%, height: h, stroke: (top: 0.3pt + black),
          inset: (left: text-at, top: pad.y, right: pad.y, bottom: if tail { 3mm } else { pad.y }), {
            place(left + horizon, dx: -text-at - mark-w / 2 + into, marks)
            if tail { place(left + bottom, dx: -text-at + 0.6mm, dy: 2.4mm, back-pt(c)) }
            align(horizon, block(width: 100%, inset: (right: pad.x - pad.y), paragraphs(ch.text)))
          })
        let floor = measure(marks).height + if tail { 6mm } else { 2mm }
        layout(size => {
          let h = measure(block(width: size.width, section(auto))).height
          section(calc.max(h + share, floor))
        })
      }
    } else {
      for lv in sections {
        block(width: 100%, fill: ps.bar, stroke: (y: 0.4pt + black), inset: (x: 1.6mm, y: 0.8mm),
          grid(columns: (1fr, auto), align: horizon,
            text(size: 0.9em, { paragraph(lv.cost.first()); [:] }),
            text(size: 0.9em, weight: "bold")[Level #lv.level]))
        fill-to(h => block(width: 100%, height: h, inset: pad, paragraphs(lv.text)))
      }
    }
    if c.flavor != none {
      block(width: 100%, inset: pad, stroke: (top: 0.3pt + black), flavor-text(c))
    }
  })))

// Which face of a two-faced card this is, on the name bar's left, where
// printed double-faced cards mark theirs (CR 712.2a–b, 712.3a–b). The marks
// are Mockery's own: a disc for a card that transforms, its chevron up on the
// front and down on the back; a tab for a modal one, one chevron on the front
// and two on the back; and for a meld card, as printed meld cards mark their
// fronts, a disc with two points meeting in its middle.
#let face-marker(kind) = {
  let chevron(dx, dy, up: true, w: 1.1mm, h: 0.65mm, paint: white) = place(top + left, dx: dx, dy: dy,
    polygon(fill: paint, stroke: none,
      ..if up { ((0mm, h), (w / 2, 0mm), (w, h), (w, h + 0.45mm), (w / 2, 0.45mm), (0mm, h + 0.45mm)) }
        else { ((0mm, 0mm), (w / 2, h), (w, 0mm), (w, 0.45mm), (w / 2, h + 0.45mm), (0mm, 0.45mm)) }))
  if kind == "meld-front" {
    box(width: 3.4mm, height: 3.4mm, baseline: 20%, {
      place(circle(radius: 1.7mm, fill: luma(30), stroke: 0.4pt + white))
      place(polygon(fill: white, stroke: none, (0.55mm, 1.0mm), (1.6mm, 1.7mm), (0.55mm, 2.4mm)))
      place(polygon(fill: white, stroke: none, (2.85mm, 1.0mm), (1.8mm, 1.7mm), (2.85mm, 2.4mm)))
    })
  } else if kind == "transform-front" or kind == "transform-back" {
    box(width: 3.4mm, height: 3.4mm, baseline: 20%, {
      place(circle(radius: 1.7mm, fill: luma(30), stroke: 0.4pt + white))
      chevron(1.15mm, 1.05mm, up: kind == "transform-front")
    })
  } else {
    // A tab, round at the left and pointed at the right.
    let front = kind == "modal-front"
    let ink = if front { white } else { luma(30) }
    box(width: 4.6mm, height: 3.2mm, baseline: 20%, {
      place(polygon(fill: if front { luma(30) } else { white }, stroke: 0.4pt + luma(30),
        (1.6mm, 0mm), (3.4mm, 0mm), (4.6mm, 1.6mm), (3.4mm, 3.2mm), (1.6mm, 3.2mm), (0mm, 1.6mm)))
      let arrow(dx) = place(top + left, dx: dx, dy: 0.9mm, polygon(fill: ink, stroke: none,
        (0mm, 0mm), (0.75mm, 0.7mm), (0mm, 1.4mm), (0mm, 1.0mm), (0.35mm, 0.7mm), (0mm, 0.4mm)))
      if front { arrow(1.95mm) } else { arrow(1.45mm); arrow(2.35mm) }
    })
  }
}

// A modal face's hint bar: what the other face is, its subtypes or types
// and its cost or, for a land, the mana it adds (CR 712.3c), at the foot's
// left behind a chevron pointing off the card. Dark on the front and light on
// the back, as the face markers are.
#let hint-bar(hint, front) = context {
  let (bg, fg) = if front { (luma(35), white) } else { (luma(235), black) }
  set text(size: 6pt, fill: fg)
  let arrow = box(baseline: -0.2em, polygon(fill: fg, stroke: none, (1mm, 0mm), (0mm, 0.7mm), (1mm, 1.4mm)))
  let what = text(weight: "bold", hint.type)
  let pays = if hint.mana != none { paragraph(hint.mana) } else { for s in hint.cost { sym(s, 6.2pt) } }
  let inset = (left: 1.2mm, right: 1.8mm)
  // As wide as Emeria's Call's, 43% of the card, its type at the left and its
  // cost at the right, or wider for a longer hint.
  let natural = measure(stack(dir: ltr, spacing: 1.4mm, arrow, what, pays)).width + 3mm + inset.left + inset.right
  box(width: calc.max(27mm, natural), height: 3.1mm, fill: bg, stroke: 0.4pt + black, radius: (right: 1.5mm),
    inset: inset, align(horizon, grid(columns: (auto, auto, 1fr, auto), column-gutter: 1.4mm, align: horizon,
      arrow, what, [], pays)))
}

// A planeswalker below its name, in the proportions of a printed one's
// frame: a barrel-shaped art window, the type line, and a text box narrower
// than the frame in a band that arches over the card's foot, its loyalty
// badges standing out over the band on its left and the loyalty shield over
// its foot's right corner. The art runs on behind a pale wash in the text
// box, as printed planeswalkers' does. A planeswalker
// of four abilities or more takes a shorter window and a taller box, as
// printed ones do; three or fewer keep a window near a creature's (CR 606).
#let walker-body(c, ps, type-bar, foot-right) = layout(size => {
  let r = c.rules
  let gap = 0.9mm
  // The window's foot where printed ones' is: 55.4% of the card's height
  // with three abilities or fewer, 48.4% with four or more.
  let art-h = if r.abilities.len() >= 4 { 33.85mm } else { 40.05mm }
  // Measured on printed planeswalkers, from the frame's inner edges: the box
  // starts 3.1mm in from the left and 0.5mm from the right, the badges are
  // centred 1.3mm outside it, the box's foot is 0.75mm above the frame's,
  // and the shield's horns 2.9mm above the box's foot, so the shield stands
  // out over the black band under the frame. The band flares into its
  // shoulder, 2.1mm above the box's foot.
  let (inl, inr) = (3.1mm, 0.5mm)
  let (sw, sh) = walker-shield
  let box-bottom = size.height - (layout-of("planeswalker").foot - layout-of("planeswalker").box-bottom)
  let shield-foot = size.height - box-bottom + shield-rise - sh
  let shoulder = box-bottom - 2.1mm
  let box-top = art-h + gap + measure(block(width: size.width, type-bar)).height + gap
  let box-h = box-bottom - box-top
  let bw = size.width - inl - inr
  // The window's sides at its widest, mid-height.
  let side-mid = 0.38mm
  // Printed walkers show their art in the window as Scryfall's art_crop of
  // them has it: as wide as the window at its widest, from the window's top
  // to its foot (measured on the six walkers above, each art_crop landing on
  // the card at its own size). The art runs on behind the text box. So a
  // user's art, which is an illustration and not a crop of the window and the
  // box together, is fitted to the window's width and hung from its top,
  // cropped only if it would not fill the window; behind the box, where it
  // runs out, it carries on mirrored, which under the wash reads as the art
  // going on.
  let art = if c.art != none {
    let natural = measure(image(c.art))
    let s = calc.max((size.width - 2 * side-mid) / natural.width, art-h / natural.height)
    let (iw, ih) = (natural.width * s, natural.height * s)
    let at = (size.width - iw) / 2
    block(width: size.width, height: box-bottom, clip: true, {
      place(top + left, dx: at, image(c.art, width: iw, height: ih))
      let below = ih
      while below < box-bottom {
        place(top + left, dx: at, dy: below, scale(y: -100%, image(c.art, width: iw, height: ih)))
        place(top + left, dx: at, dy: below + ih, image(c.art, width: iw, height: ih))
        below += 2 * ih
      }
    })
  } else {
    let k = if c.frame.tint.len() > 0 { palette.at(c.frame.tint.first()) } else { palette.at(c.frame.base) }
    rect(width: size.width, height: box-bottom, stroke: none,
      fill: gradient.linear(k.darken(35%), k.lighten(10%), angle: 60deg))
  }
  block(width: size.width, height: size.height, {
  block(width: 100%, height: art-h, below: gap, {
    let (w, h) = (size.width, art-h)
    // The window's sides bow out, so it is narrowest at its rounded top and
    // bottom corners and widest at mid-height; a band of the bars' rim colour
    // runs round it, pinched into a neck under each corner that flares into
    // the name bar above and the type line below. Measured on printed
    // planeswalkers as fractions of the card's width from its edge, which
    // these match: the window's sides 9.5-10% at its corners and 7% at
    // mid-height, the band's outer edge 5.5% at mid-height, and the necks
    // 7% at the top and 6.6% at the foot, where the band runs into the bars'
    // rims. The band is in the rims' paint, as printed bands are.
    let (side-end, corner) = (2.2mm, 1.3mm)
    let (neck, neck-foot, neck-at, foot-at) = (0.3mm, 0.05mm, 1.4mm, 1.1mm)
    let bulge(end, end2, mid) = (8 * mid - end - end2) / 6
    let side-c = bulge(side-end, side-end, side-mid)
    let band-c = bulge(neck, neck-foot, -0.5mm)
    // Both sides of a shape, the right mirroring the left, `x` in from the
    // left edge.
    let mirror(x) = w - x
    let window(fill) = curve(fill: fill, stroke: 0.4pt + black,
      curve.move((side-end + corner, 0mm)),
      curve.line((mirror(side-end + corner), 0mm)),
      curve.quad((mirror(side-end), 0mm), (mirror(side-end), corner)),
      curve.cubic((mirror(side-c), corner + (h - 2 * corner) / 3),
        (mirror(side-c), h - corner - (h - 2 * corner) / 3), (mirror(side-end), h - corner)),
      curve.quad((mirror(side-end), h), (mirror(side-end + corner), h)),
      curve.line((side-end + corner, h)),
      curve.quad((side-end, h), (side-end, h - corner)),
      curve.cubic((side-c, h - corner - (h - 2 * corner) / 3),
        (side-c, corner + (h - 2 * corner) / 3), (side-end, corner)),
      curve.quad((side-end, 0mm), (side-end + corner, 0mm)),
      curve.close(mode: "straight"))
    // The band's outer edge down one side: out of the name bar's foot, into
    // the neck, bowing out and in again, and out into the type line's head.
    let (head, foot) = (-gap + 0.07mm, h + gap - 0.07mm)
    let (y1, y2) = (neck-at, h - foot-at)
    let outer(m) = (
      curve.cubic((m(neck - 0.3mm), head), (m(neck), y1 - 0.8mm), (m(neck), y1)),
      curve.cubic((m(band-c), y1 + (y2 - y1) / 3), (m(band-c), y2 - (y2 - y1) / 3), (m(neck-foot), y2)),
      curve.cubic((m(neck-foot), y2 + 0.9mm), (m(neck-foot - 0.4mm), foot), (m(0.4mm), foot)),
    )
    let same(x) = x
    // The same edge walked upwards, to close the band's fill.
    let upward(m) = (
      curve.cubic((m(neck-foot - 0.4mm), foot), (m(neck-foot), y2 + 0.9mm), (m(neck-foot), y2)),
      curve.cubic((m(band-c), y2 - (y2 - y1) / 3), (m(band-c), y1 + (y2 - y1) / 3), (m(neck), y1)),
      curve.cubic((m(neck), y1 - 0.8mm), (m(neck - 0.3mm), head), (m(0.4mm), head)),
    )
    place(top + left, curve(fill: ps.rim, stroke: none,
      curve.move((0.4mm, head)), curve.line((mirror(0.4mm), head)),
      ..outer(mirror), curve.line((0.4mm, foot)), ..upward(same), curve.close(mode: "straight")))
    for m in (same, mirror) {
      place(top + left, curve(stroke: 0.4pt + black, curve.move((m(0.4mm), head)), ..outer(m)))
    }
    // Only a box clips, and only to a rounded rectangle, so the window is
    // filled with the art as a tiling laid from the window's own corner.
    place(top + left, window(tiling(size: (w, h), relative: "parent", art)))
  })
  block(below: gap, type-bar)

  // The text box sits in a band of the bars' rim colour, as the window does:
  // flaring out of the type line's foot, straight down the sides, round at
  // its foot's left corner, arched over the middle of the card's foot, and
  // flaring into the loyalty shield at the right, which stands over the
  // box's corner. Measured on printed planeswalkers as fractions of the
  // card: the box's foot at 91.4% of its height with the band 0.8% below
  // it, the band's sides 5.8% of its width in from its edges, its foot's
  // corner some 4mm round, and the arch 15% of the width across, rising 1%
  // of the height.
  let (w, H) = (size.width, size.height)
  // The box's foot rounds off on the left in a wide curve, as Teferi's does.
  let radius = (top-left: 1.3mm, top-right: 0.8mm, bottom-left: 4mm, bottom-right: 1mm)
  let (side, band-foot, round) = (-0.45mm, 0.9mm, 3.8mm)
  let (mid, arch, rise) = (w / 2, 4.6mm, 0.9mm)
  let head = box-top - gap + 0.07mm
  let foot = box-bottom + band-foot
  let (shield-top, shield-left) = (H - shield-foot - sh, w + 0.8mm - sw)
  // The foot's edge from right to left at `y`, bowed up over the middle.
  let bow-at(y, m) = (
    curve.line((m + arch, y)),
    curve.cubic((m + arch / 2, y), (m + arch / 2, y - rise), (m, y - rise)),
    curve.cubic((m - arch / 2, y - rise), (m - arch / 2, y), (m - arch, y)),
  )
  let bow(y) = bow-at(y, mid)
  place(top + left, curve(fill: ps.rim, stroke: 0.4pt + black,
    curve.move((0.4mm, head)),
    curve.line((w - 0.4mm, head)),
    curve.cubic((w - 0.35mm, head + 0.7mm), (w - side, head + 0.6mm), (w - side, head + 1.6mm)),
    curve.line((w - side, shoulder - 1.6mm)),
    // Into the shield's shoulder, then out of its side under the box.
    curve.cubic((w - side, shoulder - 0.6mm), (w - 0.2mm, shoulder - 0.2mm), (w - 0.9mm, shoulder + 0.2mm)),
    curve.line((shield-left + sw / 2, shield-top + sh / 2)),
    curve.line((shield-left + 1.2mm, foot + 0.3mm)),
    curve.cubic((shield-left + 0.2mm, foot), (shield-left - 0.9mm, foot), (shield-left - 1.8mm, foot)),
    ..bow(foot),
    curve.line((side + round, foot)),
    curve.cubic((side + round * 0.45, foot), (side, foot - round * 0.45), (side, foot - round)),
    curve.line((side, head + 1.6mm)),
    curve.cubic((side, head + 0.6mm), (0.35mm, head + 0.7mm), (0.4mm, head)),
    curve.close(mode: "straight")))
  // The frame stops where the band sweeps into the black band under it:
  // from the frame's edge 8mm above the band's foot, down and round in a
  // curve that runs out flat 9.5mm in, and on the right from 2.9mm above
  // the shield's shoulder into the band's side over it, as Teferi, Hero of
  // Dominaria's does. Black over the frame and the band beyond those
  // curves, under the box and the shield.
  let edge = -1.5mm - 0.1mm
  place(top + left, curve(fill: black, stroke: none,
    curve.move((edge, foot - 7.95mm)),
    curve.cubic((edge, foot - 2.96mm), (edge + 2.96mm, foot), (edge + 9.47mm, foot)),
    curve.line((edge + 9.47mm, H + 1mm)), curve.line((edge, H + 1mm)), curve.close(mode: "straight")))
  place(top + left, curve(fill: black, stroke: none,
    curve.move((w - edge, shoulder - 2.9mm)),
    curve.cubic((w - edge, shoulder - 1.6mm), (w - side, shoulder - 1.4mm), (w - side, shoulder - 0.2mm)),
    curve.line((w - side, H + 1mm)), curve.line((w - edge, H + 1mm)), curve.close(mode: "straight")))
  // The box's outline, its foot bowed as the band's is, `x` and `y` from its
  // own corner.
  let outline(fill: none) = curve(fill: fill, stroke: 0.4pt + black,
    curve.move((radius.top-left, 0mm)),
    curve.line((bw - radius.top-right, 0mm)),
    curve.quad((bw, 0mm), (bw, radius.top-right)),
    curve.line((bw, box-h - radius.bottom-right)),
    curve.quad((bw, box-h), (bw - radius.bottom-right, box-h)),
    ..bow-at(box-h, mid - inl),
    curve.line((radius.bottom-left, box-h)),
    curve.quad((0mm, box-h), (0mm, box-h - radius.bottom-left)),
    curve.line((0mm, radius.top-left)),
    curve.quad((0mm, 0mm), (radius.top-left, 0mm)),
    curve.close(mode: "straight"))
  let (badge, colon, text-at, pad) = (6mm, 2.9mm, 4mm, 1.4mm)
  let bottom-in = sh - 2mm
  place(top + left, dx: inl, dy: box-top, block(width: bw, height: box-h, {
    place(top + left, block(width: 100%, height: 100%, radius: radius, clip: true, {
      place(dx: -inl, dy: -box-top, art)
      place(top + left, rect(width: 100%, height: 100%, stroke: none, fill: see-through(ps.box, 18%)))
    }))
    place(top + left, outline())
    block(width: 100%, height: 100%, inset: (left: pad, right: 1.6mm, top: 1.3mm, bottom: bottom-in),
      fit-box(floor: floor-of(c), (sz, spare) => text-style(sz, {
        // Static abilities across the box; each loyalty ability a band of
        // its own, shaded in turn, and the bands sharing the height left
        // over, as printed planeswalkers' text boxes are (CR 606).
        set block(spacing: 0mm)
        let bands = r.abilities.filter(a => a.loyalty != none).len()
        let share = spare / calc.max(bands, 1)
        let n = 0
        let last = r.abilities.len() - 1
        for (i, a) in r.abilities.enumerate() {
          if a.loyalty == none { block(above: 1.2mm, below: 1.2mm, paragraphs(a.text)) } else {
            let shade = if calc.rem(n, 2) == 0 { black.transparentize(92%) } else { white.transparentize(60%) }
            n += 1
            let (first, tail) = (i == 0, i == last and c.flavor == none)
            let band(h) = block(width: 100%, height: h, breakable: false, fill: shade,
              // The first and last bands run to the box's edges, round
              // where it is.
              radius: (top-left: if first { radius.top-left } else { 0mm },
                top-right: if first { radius.top-right } else { 0mm },
                bottom-left: if tail { radius.bottom-left } else { 0mm },
                bottom-right: if tail { radius.bottom-right } else { 0mm }),
              outset: (left: pad, right: 1.6mm, top: if first { 1.3mm } else { 0mm },
                bottom: if tail { bottom-in } else { 0mm }),
              above: 0mm, below: 0mm,
              inset: (left: text-at - pad, y: 0.9mm), {
                place(left + horizon, dx: -text-at - 1.3mm - badge / 2, loyalty-badge(a.loyalty.cost, a.loyalty.sign))
                place(left + horizon, dx: colon - text-at, text(weight: "bold", [:]))
                align(horizon, paragraphs(a.text))
              })
            // No band is shallower than its badge with a margin round it,
            // so badges never crowd each other as printed ones never do. A
            // melded card's margin is as much smaller as its text floor:
            // Urza, Planeswalker's five badges stand 6.8mm apart printed,
            // closer than a card's grown would.
            let least = calc.max(3.7mm + 0.6mm, (3.7mm + 2.2mm) * (floor-of(c) / 5pt))
            layout(size => band(calc.max(measure(band(auto), width: size.width).height, least) + share))
          }
        }
        if c.flavor != none {
          block(above: 1.2mm, below: 1.2mm, line(length: 100%, stroke: 0.3pt))
          flavor-text(c)
        }
      })))
    // The band again over the arch, where the box's corners and shading
    // cannot follow it.
    let m = mid - inl
    place(top + left, curve(fill: ps.rim, stroke: none,
      curve.move((m + arch, box-h + 0.2mm)), ..bow-at(box-h, m).slice(1), curve.close(mode: "straight")))
    place(top + left, curve(stroke: 0.4pt + black, curve.move((m + arch, box-h)), ..bow-at(box-h, m).slice(1)))
  }))
  if foot-right != none { place(bottom + right, dx: 0.8mm, dy: -shield-foot, foot-right) }
})
})

// A name or type bar's plate, `w` by `h` for what it holds, in the shape of
// printed modern frames' bars: wider than the art and the text box, each end
// a half ellipse bulging out past them, under a rim in the card's colour, and
// filled lighter at the top than the foot over a shadow along its foot and
// left end, so it reads as raised. An end that is "point" is a fused split
// half's, pointing across the gutter at the other half (CR 702.102a), and
// one that is "flat" a prepare spell's, standing against the card's own text
// box.
// Measured on printed cards at 745 px wide: the rim's outer edge 4.7% of the
// width in from the card's, the fill's end bulging some 12 px, the rim 6 px.
#let bar-depth = 1.05mm
#let bar-rim = 0.55mm
// A colour, or a gradient across colours, with `f` applied to each.
#let shade(paint, f) = if type(paint) == gradient {
  gradient.linear(..paint.stops().map(((c, at)) => (f(c), at)), angle: paint.angle())
} else { f(paint) }
#let plate(w, h, ps, ends: ("round", "round"), over: 0.5mm) = {
  let k = 0.5523
  // The outline grown by `g` past the fill's, clockwise from the top left.
  let outline(g) = {
    let (x0, x1, top, foot, mid) = (if ends.first() == "flat" { -g } else { -over - g }, w + over + g, -g, h + g, h / 2)
    let rx = bar-depth + g / 2
    let ry = h / 2 + g
    let (l, r) = ends
    let (lx, rx-at) = (x0 + rx, x1 - rx)
    let (pa, pout) = (1.6mm, 1mm)
    let segs = (curve.move((if l == "point" { pa - over } else if l == "flat" { x0 } else { lx }, top)),)
    if r == "point" {
      segs += (curve.line((w - pa + over, top)), curve.line((w + pout + g * 1.3, mid)),
        curve.line((w - pa + over, foot)))
    } else {
      segs += (curve.line((rx-at, top)),
        curve.cubic((rx-at + k * rx, top), (x1, mid - k * ry), (x1, mid)),
        curve.cubic((x1, mid + k * ry), (rx-at + k * rx, foot), (rx-at, foot)))
    }
    if l == "point" {
      segs += (curve.line((pa - over, foot)), curve.line((-pout - g * 1.3, mid)),
        curve.line((pa - over, top)))
    } else if l == "flat" {
      segs += (curve.line((x0, foot)), curve.line((x0, top)))
    } else {
      segs += (curve.line((lx, foot)),
        curve.cubic((lx - k * rx, foot), (x0, mid + k * ry), (x0, mid)),
        curve.cubic((x0, mid - k * ry), (lx - k * rx, top), (lx, top)))
    }
    segs + (curve.close(mode: "straight"),)
  }
  let shape(g, ..style) = place(top + left, curve(..style, ..outline(g)))
  // A soft shadow under the plate, then the rim, outlined dark inside and out.
  place(top + left, dy: 0.3mm, curve(fill: black.transparentize(70%), stroke: none, ..outline(bar-rim + 0.1mm)))
  shape(bar-rim, fill: ps.rim, stroke: 0.9pt + luma(25))
  shape(0mm, fill: shade(ps.bar, c => c.darken(28%)), stroke: none)
  // The fill shifted up and right off its own shadow, the bevel. A bar
  // blended across, as a fuse bar is, keeps its blend and loses the bevel's
  // shading down it, which one gradient cannot hold as well.
  place(top + left, dx: 0.22mm, dy: -0.25mm, curve(stroke: none,
    fill: if type(ps.bar) == gradient { shade(ps.bar, c => c.lighten(10%)) }
      else { gradient.linear(ps.bar.lighten(20%), ps.bar, ps.bar.darken(12%), angle: 90deg) },
    ..outline(-0.18mm)))
  shape(0mm, fill: none, stroke: 0.45pt + luma(25))
}

// The deep tone an inset spell's bars are filled in, by its colour, with
// white type on them, as printed Adventures' and prepare spells' bars are:
// Stomp's red, Swift End's black, Chop Down's a warm grey for white. A spell
// of two colours or more is the two blended on an Adventure (Bring Back) and
// silver on a prepare spell (Heroic Stanza, Pest Friend).
#let deep = (
  white: rgb("#968b78"),
  blue: rgb("#2c6aa6"),
  black: rgb("#3a3536"),
  red: rgb("#b13d29"),
  green: rgb("#2e6c45"),
)
#let spell-names(frame) = if frame.base in deep { (frame.base,) } else if frame.base == "gold" { frame.at("colors", default: ()) } else { () }
#let spell-tone(frame, blend: true) = {
  let names = spell-names(frame)
  if names.len() == 1 { deep.at(names.first()) }
  else if names.len() == 2 and blend { gradient.linear(..names.map(k => deep.at(k))) }
  else { rgb("#77787c") }
}

// Text in half a text box tops out smaller than a whole box's: measured on
// Lovestruck Beast, Adventurous Eater and Blazing Firesinger, whose half
// boxes have room to spare, their lines stand 31-34 px apart at 745 px wide,
// an x-height of Libertinus at 8.5pt.
#let half-max = 8.5pt

// A paper paint shaded toward the book's spine, `toward` the side it is on.
#let page-paint(box, toward) = if type(box) == color {
  let (a, b) = (box, box.darken(9%))
  gradient.linear(..if toward == "right" { (a, a, b) } else { (b, a, a) })
} else { box }

// An inset spell's name and cost, and its type line, set in white bold.
#let spell-name(s, size) = grid(columns: (1fr, auto), column-gutter: 1mm, align: horizon,
  fit-line(s.name, size, 5pt, "the spell's name"), { for x in s.cost { sym(x, size * 0.95) } })
#let spell-type(s, size) = fit-line(s.type_line, size, 5pt, "the spell's type line")

// An Adventure: the text box is an open book, the Adventure on its left page
// under its own name and type bars in the spell's colour, and the card's own
// text on the right (CR 715.1–715.2). Measured on Bonecrusher Giant, Murderous
// Rider, Brazen Borrower, Lovestruck Beast, Giant Killer and Gingerbread
// Hunter at 745 by 1040 px: the pages meet at the box's middle, the name and
// type bars each 45 px (3.8mm) deep across the left page's top, and the
// spell's text set as large as the card's.
#let adventure-text(c, ps) = {
  let s = c.inset.spell
  let tone = spell-tone(s.frame)
  let bar-h = 3.8mm
  let size = 7.5pt
  block(width: 100%, height: 1fr, grid(columns: (1fr, 1fr), rows: (100%,), column-gutter: 0mm,
    block(width: 100%, height: 100%, fill: page-paint(ps.box, "right"), stroke: 0.4pt + black,
      radius: (top-left: 0.5mm, top-right: 1.6mm), clip: true, {
      set block(spacing: 0mm)
      set text(fill: white)
      // The name bar lighter than the type bar under it, as Stomp's is.
      block(width: 100%, height: bar-h, fill: tone, stroke: (bottom: 0.4pt + black),
        block(width: 100%, height: 100%, fill: white.transparentize(84%), inset: (left: 1.3mm, right: 1mm),
          align(horizon, spell-name(s, size))))
      block(width: 100%, height: bar-h, fill: tone, stroke: (bottom: 0.4pt + black),
        block(width: 100%, height: 100%, fill: black.transparentize(68%), inset: (x: 1.3mm),
          align(horizon, spell-type(s, size))))
      set text(fill: black)
      standard-text(s, paints(s.frame), max: half-max, height: 1fr, fill: none, stroke: none, clear: 1.6mm, centre-lone: false)
    }),
    standard-text(c, ps, max: half-max, height: 100%, fill: page-paint(ps.box, "left"), centre-lone: false),
  ))
}

// A prepare spell: the card's own text in the left of its text box, and the
// spell in a frame inset on the right (CR 722.1–722.2), its name and type on
// bars shaped as the card's are, flat against the card's text and round at
// the frame's edge, in the spell's colour, over a text box of its own that
// stops short of the card's foot, leaving the frame for the power and
// toughness. Measured on Adventurous Eater, Blazing Firesinger, Abigale, Poet
// Laureate, Emeritus of Ideation and Lluwen, Exchange Student at 745 by 1040
// px: the bars' fills 42 px (3.55mm) deep and 8 px apart, starting 4 px under
// the card's box, the spell's box from 10 px under them to 38 px above the
// card's box's foot, and the halves meeting at the box's middle.
#let prepare-text(c, ps) = {
  let s = c.inset.spell
  let tone = spell-tone(s.frame, blend: false)
  let bar-h = 3.55mm
  let size = 8pt
  let bar(body, paint) = layout(size => block(width: 100%, {
    plate(size.width, bar-h, (rim: paint.darken(35%), bar: paint), ends: ("flat", "round"))
    block(width: 100%, height: bar-h, inset: (left: 1.2mm, right: 1.3mm), align(horizon, body))
  }))
  block(width: 100%, height: 1fr, grid(columns: (1fr, 1fr), rows: (100%,), column-gutter: 0.8mm,
    standard-text(c, ps, max: half-max, height: 100%, clear: 1.8mm, centre-lone: false),
    block(width: 100%, height: 100%, {
      set block(spacing: 0mm)
      v(bar-rim - 0.1mm)
      {
        set text(fill: white)
        bar(spell-name(s, size), tone.lighten(12%))
        v(0.7mm)
        bar(spell-type(s, size), tone.darken(18%))
      }
      v(0.85mm)
      let sps = paints(s.frame)
      standard-text(s, sps, max: half-max, height: 1fr, top: 1mm, clear: 1mm, centre-lone: false,
        fill: if type(sps.box) == color { sps.box.darken(14%) } else { sps.box })
      v(3.25mm)
    }),
  ))
}

// An Omen: the spell in a frame inset on the left of the text box and the
// card's own text on the right (CR 720.1–720.2), the frame in the spell's
// colour holding its name and type on bars of the card's shape, the name
// bar lighter than the type bar, over a box of its own tinted with the
// spell's colour that runs to the foot of the card's. Measured on Marang
// River Regent, Twinmaw Stormbrood, Sagu Wildling and Scavenger Regent at
// 745 by 1040 px: the halves meet at the box's middle, the bars' fills 40 px
// (3.4mm) deep and 6 px apart, the name bar's 3 px under the card's box.
#let omen-text(c, ps) = {
  let s = c.inset.spell
  let tone = spell-tone(s.frame)
  let solid = if type(tone) == color { tone } else { rgb("#77787c") }
  let bar-h = 3.4mm
  let size = 8pt
  let bar(body, paint) = layout(size => block(width: 100%, {
    plate(size.width, bar-h, (rim: solid.darken(30%), bar: paint), over: 0.3mm)
    block(width: 100%, height: bar-h, inset: (left: 1.1mm, right: 0.9mm), align(horizon, body))
  }))
  let sps = paints(s.frame)
  let tint = shade(tone, c => color.mix((paper, 68%), (c, 32%)))
  block(width: 100%, height: 1fr, grid(columns: (1fr, 1fr), rows: (100%,), column-gutter: 0.6mm,
    block(width: 100%, height: 100%, fill: solid.lighten(32%), stroke: 0.4pt + black,
      inset: (left: 0.5mm, right: 0.6mm, top: 0.45mm, bottom: 0.6mm), {
      set block(spacing: 0mm)
      {
        set text(fill: white)
        bar(spell-name(s, size), shade(tone, c => c.lighten(30%)))
        v(0.5mm)
        bar(spell-type(s, size), shade(tone, c => c.darken(28%)))
      }
      v(0.75mm)
      standard-text(s, sps, max: half-max, height: 1fr, top: 1mm, clear: 1mm, centre-lone: false,
        fill: tint, stroke: 0.4pt + solid.darken(45%))
    }),
    standard-text(c, ps, max: half-max, height: 100%, centre-lone: false),
  ))
}

// One face's body, filling the box it is given: a card's whole face, or
// half a card. `g` puts its parts, and a half's says how far its bars stand
// out (`over`), its type line's size and its text's largest. A fused half's
// name and type bars `point` at the other half, as printed fuse cards' do.
#let face-body(c, g, point: none) = {
  let ps = paints(c.frame)
  set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
  // The face's parts stand where `g` puts them, so nothing between them is
  // left to block spacing.
  set block(spacing: 0mm)
  // The plate stands out past the face's own edge, as printed bars do past
  // the art and the text box; a split half's less far, to stay in its half.
  let over = g.at("over", default: 0.5mm)
  let bar(body, h) = layout(size => {
    // A pointed end's slant starts 1.6mm in, so the text keeps clear of it.
    let inner = block(width: 100%, height: h, align(horizon, body), inset: (
      left: if point == "left" { 3.1mm } else { 1.5mm },
      right: if point == "right" { 3.1mm } else { 1.5mm }))
    let ends = if point == "right" { ("round", "point") } else if point == "left" { ("point", "round") } else { ("round", "round") }
    block(width: 100%, { plate(size.width, h, ps, ends: ends, over: over); inner })
  })
  // A split half's type line is set smaller, in its shallower bar.
  let type-size = g.at("type-size", default: 8.5pt)
  let type-bar(room: 0mm) = bar(pad(right: room, if c.indicator.len() > 0 {
    grid(columns: (auto, 1fr), column-gutter: 1.2mm, align: horizon,
      indicator(c.indicator), fit-line(c.type_line, type-size, 5pt, "the type line"))
  } else { fit-line(c.type_line, type-size, 5pt, "the type line") }), g.type-h)
  // The corner, with a transforming front's reminder of its back's beside it.
  let foot-right = if c.corner != none and c.back_pt != none {
    stack(dir: ltr, spacing: 1mm, move(dy: -1.6mm, back-pt(c)), corner-box(c, ps))
  } else if c.corner != none { corner-box(c, ps) } else if c.back_pt != none {
    move(dy: -1.3mm, back-pt(c))
  } else { none }
  // A modal face's hint bar over the foot of the text box, its left end at
  // the frame's edge and its foot just above the frame's, as Emeria's Call's
  // is.
  let hint = if c.hint != none {
    place(bottom + left, dx: -1.5mm, dy: -0.4mm, hint-bar(c.hint, c.marker == "modal-front"))
  }

  // The name gives way to the cost, never the other way round.
  bar(grid(columns: if c.marker != none { (auto, 1fr, auto) } else { (1fr, auto) },
    column-gutter: 1mm, align: horizon,
    ..if c.marker != none { (face-marker(c.marker),) },
    fit-line(c.name, 9.5pt, 6pt, "the name"),
    { for s in c.cost { sym(s, 9pt) } },
  ), g.name-h)
  v(g.art-top - g.name-top - g.name-h)

  let r = c.rules
  if r.kind == "saga" or r.kind == "class" {
    // The art stands down one side and the type line goes to the foot:
    // on the right of a Saga, the left of a Class (CR 714.1, 716.1).
    let text-col = column-text(c, ps)
    let art-col = art-box(c, ps)
    block(width: 100%, height: 1fr, grid(columns: if r.kind == "saga" { (54%, 1fr) } else { (1fr, 54%) },
      column-gutter: 0.9mm, rows: (100%,),
      ..if r.kind == "saga" { (text-col, art-col) } else { (art-col, text-col) }))
    v(g.type-top - g.art-bottom)
    // A Saga creature's other abilities, and its power and toughness, sit
    // in a text box below the type line (CR 714.1a). With no abilities
    // there the box would be empty, so the corner rides on the type line.
    let after = if r.kind == "saga" { r.after } else { () }
    let boxed = after.len() > 0
    // A transforming Saga's reminder of its back's power and toughness rides
    // on the type line's end, the corner being the back's.
    let reminder = c.back_pt != none and c.corner == none and r.kind != "saga"
    // Under the type line, the frame's foot, or room for a hint bar; a
    // split half's Saga keeps a millimetre over its half's foot.
    let under = if c.hint != none { 4mm } else if "foot" in g { g.foot - g.type-top - g.type-h } else { 1mm }
    type-bar(room: if c.corner != none and not boxed { 11mm } else if reminder { 7mm } else { 0mm })
    if reminder { place(bottom + right, dx: -1.6mm, dy: -under - g.type-h / 2 + 0.75mm, back-pt(c)) }
    if boxed {
      v(0.9mm)
      block(width: 100%, height: 9mm, fill: ps.box, stroke: 0.4pt + black,
        inset: (left: 2mm, right: if c.corner != none { 12mm } else { 2mm }, y: 1.2mm),
        fit-box((sz, _) => text-style(sz, align(center + horizon, paragraphs(after))),
          what: "the text below the type line", floor: floor-of(c)))
    }
    v(under)
    if foot-right != none and not (c.back_pt != none and c.corner == none) {
      // Over the box's foot as on any card, or centred on the type line.
      place(bottom + right, dx: if boxed { 0.85mm } else { 0.6mm },
        dy: if boxed { -under + 1.9mm } else { -under - g.type-h / 2 + 2.45mm }, foot-right)
    }
    hint
  } else if r.kind == "planeswalker" {
    block(width: 100%, height: 1fr, walker-body(c, ps, type-bar(), foot-right))
    hint
  } else {
    art-box(c, ps, height: g.art-bottom - g.art-top)
    v(g.type-top - g.art-bottom)
    type-bar()
    v(g.box-top - g.type-top - g.type-h)
    // A half's text may top out smaller than a whole card's. A spell inset
    // in the card shares its text box.
    if c.inset == none { standard-text(c, ps, max: g.at("text-max", default: 9.5pt)) }
    else if c.inset.kind == "adventure" { adventure-text(c, ps) }
    else if c.inset.kind == "omen" { omen-text(c, ps) }
    else { prepare-text(c, ps) }
    hint
    if foot-right != none and not (r.kind == "station" and r.thresholds.len() > 0 and c.back_pt == none) {
      // The plate straddles the text box's bottom right corner, standing
      // 0.85mm out past its side and 1.9mm below its foot, as Lotus Cobra's
      // does, with the box's bottom inset keeping the text clear of it.
      let in-band = r.kind == "station" and r.thresholds.len() > 0
      if in-band { place(bottom + right, dx: 0.6mm, dy: 0.2mm, move(dy: -1.3mm, back-pt(c))) }
      else { place(bottom + right, dx: 0.85mm, dy: 1.9mm, foot-right) }
    }
  }
}

// Where a real card's collector line sits, in the black band under the
// frame, this says what it is: the artist on the left and that it is no
// Magic card on the right, a band `width` wide with its baseline 3mm off the
// card's edge, as a printed card's lower collector line is.
#let footer(artist, width) = block(width: width, height: 6mm, inset: (left: 2.9mm, right: 4mm), {
  set text(font: "Libertinus Serif", size: 4.4pt, fill: white)
  place(bottom + left, dy: -3mm, if artist != none [Illus. #artist] else [])
  place(bottom + right, dy: -3mm, [Unofficial fan content · not a Magic card])
})

// A face `extra` taller than a card: a meld pair's melded card, drawn a
// card wide and then scaled up. Brisela's art and type line stand lower
// than a card's grown would, its text box's foot where a card's is and the
// black band under it deeper, so the art and the type line take the extra
// height from the box and the band.
#let taller(g, extra) = {
  let out = g
  for k in ("art-bottom", "type-top", "box-top") {
    if k in g { out.insert(k, g.at(k) + extra) }
  }
  out
}

#let face-card(c, extra: 0mm) = {
  let g = taller(layout-of(c.rules.kind), extra)
  block(width: card-width, height: card-height + extra, fill: black, radius: 2.5mm, breakable: false, {
    place(top + left, dx: border-side, dy: border-top,
      block(width: card-width - 2 * border-side, height: g.foot - border-top, fill: paints(c.frame).body,
        radius: 1.2mm, inset: (x: 1.5mm, top: g.name-top - border-top,
          bottom: if "box-top" in g { g.foot - g.box-bottom } else { 0mm }),
        face-body(c, g)))
    place(bottom + left, footer(c.artist, card-width))
  })
}

// A split half, read with the card on its side, from its top: measured on
// Wear // Tear at 1040 by 745 px. The frame's top is the card's border. Its
// bars stand out less far than a whole card's, to stay in its half, and its
// type line and text are set smaller, as Wear // Tear's are: the text's
// x-height 1.18mm against a whole card's 1.48mm.
#let split-layout = (name-top: 4.5mm, name-h: 4.2mm, art-top: 9.3mm, art-bottom: 33.5mm,
  type-top: 34.3mm, type-h: 2.8mm, box-top: 38.2mm, over: 0.3mm, type-size: 6.5pt, text-max: 7.5pt)
// The black band along the card's foot, which is the split card's left
// side read on its side.
#let split-band = 6.2mm
// A fused card's Fuse bar, its fill as deep as Wear // Tear's and Far //
// Away's, 41 of 745 px.
#let fuse-h = 3.5mm

// Paints side by side, blended across: a fuse bar's or a Room's type line's,
// in each half's colour from its own side.
#let across(paints) = {
  let colors = paints.map(p => if type(p) == gradient { p.stops().map(s => s.first()) } else { (p,) }).flatten()
  if colors.len() == 1 { colors.first() } else { gradient.linear(..colors) }
}

// A split card: two halves side by side on a card turned on its side, read
// with the card turned a quarter clockwise, its left edge at the top and its
// foot on the left (CR 709.1), each half a face of its own. Halves that both
// have fuse print it once, on a bar across the foot of both (CR 702.102a),
// shaped as the name and type bars are, and their name and type bars point
// at each other. The footer runs along the card's foot as any card's does,
// which read on its side is the left.
#let split-card(c) = rotate(-90deg, reflow: true,
  block(width: card-height, height: card-width, fill: black, radius: 2.5mm, breakable: false,
    inset: (left: split-band, right: border-side, top: border-side, bottom: 3.2mm), {
    set block(spacing: 0mm)
    let fused = c.style == "fused"
    // A fused card's halves stop short of the bar, which stands over their
    // frames' foot as printed fuse bars do.
    let under = if fused { fuse-h + 2 * bar-rim + 1.5mm } else { 0.3mm }
    block(width: 100%, height: 100%, grid(columns: (1fr, 1fr), column-gutter: 1mm, rows: (100%,),
      ..c.halves.enumerate().map(((i, h)) => block(width: 100%, height: 100%, fill: paints(h.frame).body,
        radius: 1.2mm, inset: (x: 1.3mm, top: split-layout.name-top - border-side, bottom: under),
        face-body(h, split-layout, point: if not fused { none } else if i == 0 { "right" } else { "left" })))))
    if fused {
      let ps = (rim: across(c.halves.map(h => paints(h.frame).rim)), bar: across(c.halves.map(h => paints(h.frame).bar)))
      place(bottom + left, dx: 1.3mm, dy: -1.2mm, block(width: card-height - split-band - border-side - 2.6mm, height: fuse-h, {
        layout(size => plate(size.width, fuse-h, ps))
        block(width: 100%, height: 100%, inset: (x: 1.4mm), align(horizon,
          text(font: "Libertinus Serif", size: 7.5pt, fill: black, paragraph(c.shared))))
      }))
    }
    place(top + left, dx: -split-band, dy: -border-side, rotate(90deg, reflow: true, footer(c.artist, card-width)))
  }))

// An aftermath card's halves, measured on Cut // Ribbons, Never // Return,
// Spring // Mind and Commit // Memory at 745 by 1040 px, which agree to a
// pixel or two. The left half stands upright on the card's top half, a card
// face in small, its frame from `frame-top` to `foot` from the card's top
// edge; its text box is 2.1mm in from the frame's sides, not a whole card's
// 1.5mm.
#let aftermath-top = (frame-top: 2.6mm, inset: 2mm, name-top: 4.48mm, name-h: 4.65mm, art-top: 9.75mm,
  art-bottom: 29.5mm, type-top: 30.35mm, type-h: 4.15mm, box-top: 35.7mm, box-bottom: 47mm, foot: 47.4mm,
  type-size: 8pt, text-max: 9pt)
// The right half lies on its side below it, `at` from the card's top and
// `length` along it, read with the card turned a quarter anticlockwise and
// measured from the card's top as it is then read. Its frame runs from
// `frame-top` to `frame-bottom` except over its `ends`, where it steps in to
// `end-top` and `end-bottom` round the bars' rounded ends, as printed ones'
// frames do.
#let aftermath-bottom = (at: 48.6mm, length: 32.1mm, frame-top: 2.54mm, frame-bottom: 60.3mm,
  end-top: 4.23mm, end-bottom: 58.7mm, ends: (3.1mm, 2.8mm), step: 1.5mm, inset: (left: 0.75mm, right: 0.45mm),
  name-top: 4.8mm, name-h: 4.7mm, art-top: 10.3mm, art-bottom: 28.4mm, type-top: 29.3mm, type-h: 4.15mm,
  box-top: 34.5mm, box-bottom: 58.2mm, over: 0.8mm, type-size: 8pt, text-max: 9pt)

// The aftermath half's frame: full depth along its middle, stepping in over
// each end along an S, with rounded corners.
#let aftermath-frame(g, fill) = {
  let (l, a, b, c, d) = (g.length, g.end-top, g.frame-top, g.frame-bottom, g.end-bottom)
  let (e0, e1, f, r, k) = (g.ends.first(), g.length - g.ends.last(), g.step, 1.2mm, 0.5523)
  curve(fill: fill, stroke: none,
    curve.move((r, a)),
    curve.line((e0 - f, a)), curve.cubic((e0 - f / 2, a), (e0 - f / 2, b), (e0, b)),
    curve.line((e1, b)), curve.cubic((e1 + f / 2, b), (e1 + f / 2, a), (e1 + f, a)),
    curve.line((l - r, a)), curve.cubic((l - r + k * r, a), (l, a + r - k * r), (l, a + r)),
    curve.line((l, d - r)), curve.cubic((l, d - r + k * r), (l - r + k * r, d), (l - r, d)),
    curve.line((e1 + f, d)), curve.cubic((e1 + f / 2, d), (e1 + f / 2, c), (e1, c)),
    curve.line((e0, c)), curve.cubic((e0 - f / 2, c), (e0 - f / 2, d), (e0 - f, d)),
    curve.line((r, d)), curve.cubic((r - k * r, d), (0mm, d - r + k * r), (0mm, d - r)),
    curve.line((0mm, a + r)), curve.cubic((0mm, a + r - k * r), (r - k * r, a), (r, a)),
    curve.close(mode: "straight"))
}

// An aftermath card (CR 702.127): the left half upright on the top of the
// card, and the right half, the one cast from a graveyard, on its side below
// it, read with the card turned a quarter anticlockwise, its name toward the
// card's right edge. The footer is the card's own, under both.
#let aftermath-card(c) = {
  let (t, g) = (aftermath-top, aftermath-bottom)
  let (upper, lower) = (c.halves.first(), c.halves.last())
  block(width: card-width, height: card-height, fill: black, radius: 2.5mm, breakable: false, {
    place(top + left, dx: border-side, dy: t.frame-top,
      block(width: card-width - 2 * border-side, height: t.foot - t.frame-top, fill: paints(upper.frame).body,
        radius: 1.2mm, inset: (x: t.inset, top: t.name-top - t.frame-top, bottom: t.foot - t.box-bottom),
        face-body(upper, t)))
    place(top + left, dy: g.at, rotate(90deg, reflow: true, block(width: g.length, height: card-width, {
      place(top + left, aftermath-frame(g, paints(lower.frame).body))
      place(top + left, block(width: g.length, height: card-width,
        inset: (..g.inset, top: g.name-top, bottom: card-width - g.box-bottom), face-body(lower, g)))
    })))
    place(bottom + left, footer(c.artist, card-width))
  })
}

// A Room's parts, read with the card on its side, from the card's top left
// corner: measured on Ticket Booth // Tunnel of Hate, Bottomless Pool //
// Locker Room, Dazzling Theater // Prop Room and Smoky Lounge // Misty Salon
// at 1040 by 744 px. Each door is a frame of its own, `gap` apart, holding
// its name bar `bar-in` in from its sides and its text box `box-in`; between
// them the art runs across both, `art-in` in from the frame's sides, behind
// the shared type line and the reminder band under it. A door's frame rounds
// off beside its text box from `body-bottom` up, `foot-r` across and up, and
// the box runs on to the card's border.
#let room-layout = (band: 6.5mm, top: 2.45mm, right: 2.55mm, bottom: 2.6mm, gap: 1.52mm,
  name-top: 3.13mm, name-h: 4.66mm, bar-in: 2.1mm, art-in: 2.1mm, art-top: 8.46mm,
  type-top: 34.05mm, type-h: 3.05mm, type-in: (1.27mm, 0.85mm), band-top: 37.9mm, band-bottom: 43.3mm,
  box-top: 43.55mm, box-in: 1.6mm, box-rim: 0.45mm, body-bottom: 58.4mm, foot-r: (1.6mm, 5mm))

// A door's frame, `w` by `h`: round at the top, and at the foot curving in
// to the text box's sides over `foot-r`.
#let door-frame(w, h, foot-r, fill) = {
  let (r, k) = (1.2mm, 0.5523)
  let (fx, fy) = foot-r
  curve(fill: fill, stroke: none,
    curve.move((r, 0mm)),
    curve.line((w - r, 0mm)), curve.cubic((w - r + k * r, 0mm), (w, r - k * r), (w, r)),
    curve.line((w, h - fy)), curve.cubic((w, h - fy + k * fy), (w - fx + k * fx, h), (w - fx, h)),
    curve.line((fx, h)), curve.cubic((fx - k * fx, h), (0mm, h - fy + k * fy), (0mm, h - fy)),
    curve.line((0mm, r)), curve.cubic((0mm, r - k * r), (r - k * r, 0mm), (r, 0mm)),
    curve.close(mode: "straight"))
}

// A Room (CR 709.5): two doors on a card turned on its side and read as a
// split card is, each with its own name bar and text box, under one picture
// across both and one shared type line (CR 709.5a), dark and see-through
// over the art and rimmed in both doors' colours, with the doors' shared
// unlock reminder on a dark band across the card under it.
#let room-card(c) = rotate(-90deg, reflow: true, {
  let g = room-layout
  let (x0, x1) = (g.band, card-height - g.right)
  let door-w = (x1 - x0 - g.gap) / 2
  let doors = c.halves.enumerate().map(((i, h)) => (h, paints(h.frame), x0 + i * (door-w + g.gap)))
  block(width: card-height, height: card-width, fill: black, radius: 2.5mm, breakable: false, {
    set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
    set block(spacing: 0mm)
    for (h, ps, x) in doors {
      place(top + left, dx: x, dy: g.top, door-frame(door-w, g.body-bottom - g.top, g.foot-r, ps.body))
    }
    // The art: one picture across both doors, fitted to cover the window
    // from the name bars to the type line as printed Rooms' art_crop is, and
    // carried on mirrored behind the type line and the band.
    let (ax, aw) = (x0 + g.art-in, x1 - x0 - 2 * g.art-in)
    let (win-h, art-h) = (g.type-top - g.art-top, g.band-bottom - g.art-top)
    let art = c.halves.first().art
    place(top + left, dx: ax, dy: g.art-top, block(width: aw, height: art-h, clip: true, stroke: 0.4pt + black, context {
      let natural = measure(image(art))
      let s = calc.max(aw / natural.width, win-h / natural.height)
      let (iw, ih) = (natural.width * s, natural.height * s)
      let (at, y0) = ((aw - iw) / 2, (win-h - ih) / 2)
      place(top + left, dx: at, dy: y0, image(art, width: iw, height: ih))
      let (below, flip) = (y0 + ih, true)
      while below < art-h {
        place(top + left, dx: at, dy: below, if flip { scale(y: -100%, image(art, width: iw, height: ih)) } else { image(art, width: iw, height: ih) })
        below += ih
        flip = not flip
      }
    }))
    // The band: the shared reminder, white and italic on the art darkened.
    place(top + left, dx: ax, dy: g.band-top, block(width: aw, height: g.band-bottom - g.band-top,
      fill: black.transparentize(22%), inset: (x: 3mm, y: 0.6mm),
      if c.shared != none {
        fit-box(max: 6.75pt, what: "the Room's reminder", (sz, spare) => text-style(sz, {
          set text(fill: white)
          v(spare / 2)
          align(center, par(paragraph(c.shared)))
        }))
      }))
    // The shared type line.
    let (ta, tb) = g.type-in
    place(top + left, dx: x0 + ta, dy: g.type-top, block(width: x1 - x0 - ta - tb, height: g.type-h, {
      layout(size => plate(size.width, g.type-h, (rim: across(doors.map(d => d.at(1).rim)), bar: luma(30).transparentize(35%)), over: 0mm))
      block(width: 100%, height: 100%, inset: (x: 1.6mm), align(horizon,
        text(fill: white, fit-line(c.type_line, 7.5pt, 5pt, "the type line"))))
    }))
    for (h, ps, x) in doors {
      // Its name bar.
      place(top + left, dx: x + g.bar-in, dy: g.name-top, block(width: door-w - 2 * g.bar-in, height: g.name-h, {
        layout(size => plate(size.width, g.name-h, ps))
        block(width: 100%, height: 100%, inset: (x: 1.5mm), align(horizon,
          grid(columns: (1fr, auto), column-gutter: 1mm, align: horizon,
            fit-line(h.name, 9.5pt, 6pt, "the name"), { for s in h.cost { sym(s, 9pt) } })))
      }))
      // Its text box, rimmed in its colour.
      let (bw, bh) = (door-w - 2 * g.box-in, card-width - g.bottom - g.box-top)
      place(top + left, dx: x + g.box-in, dy: g.box-top, block(width: bw, height: bh, fill: ps.rim,
        stroke: 0.4pt + luma(25), inset: g.box-rim,
        standard-text(h, ps, max: 9pt, height: 100%, stroke: 0.4pt + luma(25), top: 1.2mm, clear: 1.2mm)))
    }
    place(top + left, rotate(90deg, reflow: true, footer(c.artist, card-width)))
  })
})

// Where a battle's front puts its parts, read with the card on its side:
// measured on Invasion of Ikoria, Gobakhan, Tarkir, Zendikar and Alara at
// 1040 by 745 px, which agree to a pixel or two, from the frame's top left.
// The frame is the art from edge to edge, under a name bar at its top and a
// type line and text box across its foot, both narrower than the frame by a
// strip of art either side. The black band along the left, 7mm, holds the
// footer: it is the card's foot, as a split card's is.
#let battle-layout = (
  band: 7mm, top: 2.37mm, right: 2.45mm, bottom: 2.3mm,
  // A bar's block, its fill and its rim standing 1.05mm round it.
  bar-x: 3mm, name-top: 0.68mm, name-h: 4.4mm, type-top: 34.36mm, type-h: 4.4mm,
  // The text box, rim and all.
  box-left: 2.96mm, box-right: 2.98mm, box-top: 39.77mm, box-bottom: 0.11mm, box-rim: 0.42mm,
  // The defense shield's centre from the frame's bottom right corner, and
  // the back's reminder and its tab 53% down the box.
  shield-in: 2.85mm, shield-up: 2.85mm, tab-at: 53%,
)

// The defense shield (CR 310.4a), Mockery's drawing of printed battles':
// a dark square burst into eight points, at its corners and the middles of
// its sides, under a thin light rim. Measured on the five battles above at
// 80 by 82 px: the corners square, the side points 8% further out than the
// corners' sides, and the notches between them 22% in from the square's
// edge and 29% along it.
#let defense-shield(value) = {
  let (w, h) = (6.8mm, 6.9mm)
  let rim = 0.36mm
  let (hw, hh) = (w / 2 - rim, h / 2 - rim)
  let (tip, notch-in, notch-along) = (1.08, 0.78, 0.42)
  let pt(x, y) = (w / 2 + x * hw, h / 2 + y * hh)
  // Clockwise from the top left corner.
  let points = (
    pt(-1, -1), pt(-notch-along, -notch-in), pt(0, -tip), pt(notch-along, -notch-in),
    pt(1, -1), pt(notch-in, -notch-along), pt(tip, 0), pt(notch-in, notch-along),
    pt(1, 1), pt(notch-along, notch-in), pt(0, tip), pt(-notch-along, notch-in),
    pt(-1, 1), pt(-notch-in, notch-along), pt(-tip, 0), pt(-notch-in, -notch-along),
  )
  box(width: w, height: h, {
    place(polygon(stroke: (paint: luma(115), thickness: 2 * rim + 0.5pt, join: "miter", miter-limit: 4), ..points))
    place(polygon(stroke: (paint: luma(222), thickness: 2 * rim, join: "miter", miter-limit: 4), ..points))
    place(polygon(fill: luma(20), stroke: none, ..points))
    place(center + horizon, dy: 0.25mm, text(font: "Libertinus Serif", weight: "bold", size: 11pt, fill: white, value))
  })
}

// A battle's front: a card turned on its side, read with its left edge at
// the top, as a split card is, its art filling the frame and its name across
// the top, its type line and text box over the art's foot and its defense in
// a shield on the box's bottom right corner (CR 310). A Siege's front says it
// transforms on a tab out of the box's right side, where it carries its
// back's power and toughness (CR 712.2c).
#let battle-card(c) = rotate(-90deg, reflow: true, {
  let g = battle-layout
  let ps = paints(c.frame)
  let (W, H) = (card-height - g.band - g.right, card-width - g.top - g.bottom)
  block(width: card-height, height: card-width, fill: black, radius: 2.5mm, breakable: false,
    inset: (left: g.band, right: g.right, top: g.top, bottom: g.bottom), {
    set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
    set block(spacing: 0mm)
    // The art is the frame: fitted to cover the window between the bars, as
    // printed battles' art_crop is, and carried on mirrored above and below
    // it, where the bars and the box stand over it.
    let (win-top, win-bottom) = (g.name-top + g.name-h + bar-rim, g.type-top - bar-rim)
    let win-h = win-bottom - win-top
    block(width: W, height: H, radius: 1.2mm, clip: true, fill: ps.body, context {
      if c.art != none {
        let natural = measure(image(c.art))
        let s = calc.max(W / natural.width, win-h / natural.height)
        let (iw, ih) = (natural.width * s, natural.height * s)
        let (at, y0) = ((W - iw) / 2, win-top - (ih - win-h) / 2)
        let art(flip) = if flip { scale(y: -100%, image(c.art, width: iw, height: ih)) } else { image(c.art, width: iw, height: ih) }
        place(top + left, dx: at, dy: y0, art(false))
        place(top + left, dx: at, dy: y0 - ih, art(true))
        let (below, flip) = (y0 + ih, true)
        while below < H {
          place(top + left, dx: at, dy: below, art(flip))
          below += ih
          flip = not flip
        }
      }
    })
    let bar(body, h, y) = place(top + left, dx: g.bar-x, dy: y, block(width: W - 2 * g.bar-x, {
      plate(W - 2 * g.bar-x, h, ps)
      block(width: 100%, height: h, inset: (x: 1.5mm), align(horizon, body))
    }))
    bar(grid(columns: if c.marker != none { (auto, 1fr, auto) } else { (1fr, auto) },
      column-gutter: 1mm, align: horizon,
      ..if c.marker != none { (face-marker(c.marker),) },
      fit-line(c.name, 9.5pt, 6pt, "the name"),
      { for s in c.cost { sym(s, 9pt) } },
    ), g.name-h, g.name-top)
    bar(if c.indicator.len() > 0 {
      grid(columns: (auto, 1fr), column-gutter: 1.2mm, align: horizon,
        indicator(c.indicator), fit-line(c.type_line, 8.5pt, 5pt, "the type line"))
    } else { fit-line(c.type_line, 8.5pt, 5pt, "the type line") }, g.type-h, g.type-top)

    // The text box, rimmed in the bars' colour. Its text keeps clear of the
    // back's reminder or, without one, of the shield over the corner.
    let (bw, bh) = (W - g.box-left - g.box-right, H - g.box-top - g.box-bottom)
    let tabbed = c.marker == "transform-front"
    let (tab-w, tab-h) = (2.1mm, 3.3mm)
    let tab-y = bh * g.tab-at
    place(top + left, dx: g.box-left, dy: g.box-top, block(width: bw, height: bh, {
      if tabbed {
        // The tab: the rim bent out into a point off the box's right side.
        let tab(grow, fill) = place(top + left, dx: bw - g.box-rim - grow, dy: tab-y - tab-h / 2 - grow,
          polygon(fill: fill, stroke: 0.4pt + luma(25),
            (0mm, 0mm), (tab-w + g.box-rim + 2 * grow, tab-h / 2 + grow), (0mm, tab-h + 2 * grow)))
        tab(0.6mm, ps.rim)
        tab(0mm, ps.box)
      }
      place(top + left, rect(width: bw, height: bh, fill: ps.rim, stroke: 0.4pt + luma(25)))
      place(top + left, dx: g.box-rim, dy: g.box-rim,
        standard-text(c, ps, max: 8.25pt, height: bh - 2 * g.box-rim, fill: ps.box, stroke: 0.4pt + luma(25),
          top: 1.1mm, clear: 1.2mm, pad-right: if c.back_pt != none { 5.4mm } else { 3.4mm }))
      if tabbed {
        // The tab's mouth, open into the box.
        place(top + left, dx: bw - g.box-rim - 0.2mm, dy: tab-y - tab-h / 2 + 0.25mm,
          rect(width: g.box-rim + 0.5mm, height: tab-h - 0.5mm, fill: ps.box, stroke: none))
      }
      if c.back_pt != none {
        // Set as printed battles set it: as large as the text, in grey.
        place(top + right, dx: -g.box-rim - 0.5mm, dy: tab-y - 1.5mm,
          text(size: 8pt, fill: luma(95), c.back_pt))
      }
      if c.hint != none {
        place(bottom + left, dx: -1.2mm, dy: -0.4mm, hint-bar(c.hint, c.marker == "modal-front"))
      }
    }))
    if c.corner != none {
      place(top + left, dx: W - g.shield-in - 3.4mm, dy: H - g.shield-up - 3.45mm, defense-shield(c.corner))
    }
    place(top + left, dx: -g.band, dy: -g.top, rotate(90deg, reflow: true, footer(c.artist, card-width)))
  })
})

// A flip card's parts, measured on Budoka Gardener (Commander 2018) and
// Nezumi Graverobber (Commander Anthology Volume II), the flip cards printed
// in the modern frame, at 745 by 1040 px, which agree to a pixel or two. The
// frame runs from `frame-top` to `foot`; the upright half's name bar, text
// box and type line stand over the art, from the card's top edge, and the
// flipped half's under it, measured from the card's foot as it reads turned
// upside down. A bar's top and height are its fill's.
#let flip-layout = (frame-top: 2.37mm, foot: 80.2mm, art-top: 26.15mm, art-bottom: 54.85mm,
  upright: (name-top: 3.55mm, name-h: 4.2mm, box-top: 8.46mm, box-bottom: 19.7mm, type-top: 21mm, type-h: 3.85mm),
  flipped: (name-top: 10.28mm, name-h: 3.9mm, box-top: 15.23mm, box-bottom: 27.25mm, type-top: 28.22mm, type-h: 3.81mm))

// One half of a flip card, on a card-sized sheet: its name bar, its text
// box under it and its type line under that, the power and toughness on the
// type line's right end as printed flip cards carry theirs. `ps` are the
// card's paints, the upright half's: flipping changes no colour
// (CR 710.1c).
#let flip-half(h, ps, g) = {
  set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
  set block(spacing: 0mm)
  let x = border-side + 1.5mm
  let w = card-width - 2 * x
  let bar(body, y, height) = place(top + left, dx: x, dy: y, block(width: w, height: height, {
    plate(w, height, ps)
    block(width: 100%, height: height, inset: (x: 1.5mm), align(horizon, body))
  }))
  bar(grid(columns: (1fr, auto), column-gutter: 1mm, align: horizon,
    fit-line(h.name, 9.5pt, 6pt, "the name"), { for s in h.cost { sym(s, 9pt) } }), g.name-top, g.name-h)
  place(top + left, dx: x, dy: g.box-top, block(width: w, height: g.box-bottom - g.box-top,
    // Budoka Gardener's and Nezumi Graverobber's lines stand 32.5 px apart,
    // Libertinus's at 8pt.
    standard-text(h, ps, max: 8pt, height: 100%, top: 1.2mm, clear: 1.2mm)))
  let room = if h.corner != none { 10mm } else { 0mm }
  let type-line = fit-line(h.type_line, 8.5pt, 5pt, "the type line")
  bar(pad(right: room, if h.indicator.len() > 0 {
    grid(columns: (auto, 1fr), column-gutter: 1.2mm, align: horizon, indicator(h.indicator), type-line)
  } else { type-line }), g.type-top, g.type-h)
  if h.corner != none {
    // Its right end over the bar's, centred on the bar, as Budoka
    // Gardener's and Nezumi Graverobber's power and toughness stand.
    place(top + right, dx: -x + 0.5mm, dy: g.type-top + g.type-h / 2 - 2.45mm, corner-box(h, ps))
  }
}

// A flip card (CR 710): one face, its upright half above the art and its
// flipped half below it upside down, read by turning the card round, each
// with its own name bar, text box and type line, in the card's colour.
#let flip-card(c) = {
  let g = flip-layout
  let (upright, flipped) = (c.halves.first(), c.halves.last())
  let ps = paints(upright.frame)
  block(width: card-width, height: card-height, fill: black, radius: 2.5mm, breakable: false, {
    place(top + left, dx: border-side, dy: g.frame-top, block(width: card-width - 2 * border-side,
      height: g.foot - g.frame-top, fill: ps.body, radius: 1.2mm))
    place(top + left, dx: border-side + 1.5mm, dy: g.art-top,
      block(width: card-width - 2 * border-side - 3mm, art-box(upright, ps, height: g.art-bottom - g.art-top)))
    place(top + left, block(width: card-width, height: card-height, flip-half(upright, ps, g.upright)))
    place(top + left, rotate(180deg, block(width: card-width, height: card-height, flip-half(flipped, ps, g.flipped))))
    place(bottom + left, footer(c.artist, card-width))
  })
}

// A meld pair's melded card is half an oversized card on each card's back
// (CR 712.4): two cards wide on their sides, so 88 by 126mm, drawn as a face
// a card wide and as tall as that shape is, then scaled up to it, as the
// printed ones are a card's frame grown, Brisela's and Urza's text and all.
#let melded-scale = card-height / card-width
#let melded-card(m) = scale(x: melded-scale * 100%, y: melded-scale * 100%, origin: top + left, reflow: true,
  face-card(m + (floor: 5pt / melded-scale), extra: 2 * card-width / melded-scale - card-height))

// A meld card's back: its half of the melded card, the top half on the
// back of the card that melds them and the bottom on its counterpart's, as
// printed pairs' are, read with the card turned a quarter anticlockwise,
// its right edge at the top: the other way from a split card, as Eldritch
// Moon's printed backs are turned. The two laid side by side, the top
// half's on the right, make the melded card.
#let meld-back(c) = block(width: card-width, height: card-height, fill: black, radius: 2.5mm, clip: true,
  breakable: false, rotate(90deg, reflow: true, block(width: card-height, height: card-width, clip: true,
    place(top + left, dy: if c.half == "top" { 0mm } else { -card-width }, melded-card(c.melded)))))

#let card(c) = if c.kind == "split" {
  if c.style == "aftermath" { aftermath-card(c) } else if c.style == "room" { room-card(c) } else { split-card(c) }
} else if c.kind == "battle" { battle-card(c) } else if c.kind == "flip" { flip-card(c) } else if c.kind == "meld-back" { meld-back(c) } else { face-card(c) }

#let notice = [Unofficial Fan Content permitted under the Fan Content Policy. Not approved/endorsed by Wizards. Portions of the materials used are property of Wizards of the Coast. ©Wizards of the Coast LLC. Made with Mirage Mockery.]

#if job.layout == "card" {
  set page(width: card-width, height: card-height, margin: 0mm)
  for (i, c) in job.cards.enumerate() {
    if i > 0 { pagebreak() }
    card(c)
  }
} else {
  // As many cards as the paper holds inside its margin, centred, with the
  // same parameters and marks as Phyrexian Metamorph's proxy sheet: with no
  // gap one cut serves two cards, and the marks stay outside the grid so none
  // is printed on a card.
  let sheet = job.sheet
  let gap = sheet.gap_mm * 1mm
  let margin = sheet.margin_mm * 1mm
  // The page's size comes in the job rather than from `layout`, which would
  // hold the pages in a container that cannot break.
  let page-width = sheet.width_mm * 1mm
  let page-height = sheet.height_mm * 1mm
  set page(width: page-width, height: page-height, margin: 0mm)
  {
    let fit(room, card) = calc.floor((room - 2 * margin + gap) / (card + gap))
    let cols = fit(page-width, card-width)
    let rows = fit(page-height, card-height)
    if cols < 1 or rows < 1 { panic("no card fits inside that margin") }
    let w = cols * card-width + (cols - 1) * gap
    let h = rows * card-height + (rows - 1) * gap
    let x0 = (page-width - w) / 2
    let y0 = (page-height - h) / 2
    let offset = 1mm
    let mark = 5mm
    // The notice goes outside the marks, in whichever margin has more room.
    let marks = if sheet.cut_marks { offset + mark } else { 0mm }
    let side = x0 - marks
    let foot = y0 - marks
    if calc.max(side, foot) < 2.5mm {
      panic("no margin has room for the Fan Content Policy notice; lower the margin or the gap")
    }
    let per-page = cols * rows
    for (n, start) in range(0, job.cards.len(), step: per-page).enumerate() {
      if n > 0 { pagebreak() }
      let chunk = job.cards.slice(start, calc.min(start + per-page, job.cards.len()))
      if sheet.cut_marks {
        let stroke = 0.3pt
        for i in range(cols) {
          for x in (x0 + i * (card-width + gap), x0 + i * (card-width + gap) + card-width) {
            place(top + left, dx: x, dy: y0 - offset - mark, line(length: mark, angle: 90deg, stroke: stroke))
            place(top + left, dx: x, dy: y0 + h + offset, line(length: mark, angle: 90deg, stroke: stroke))
          }
        }
        for j in range(rows) {
          for y in (y0 + j * (card-height + gap), y0 + j * (card-height + gap) + card-height) {
            place(top + left, dx: x0 - offset - mark, dy: y, line(length: mark, stroke: stroke))
            place(top + left, dx: x0 + w + offset, dy: y, line(length: mark, stroke: stroke))
          }
        }
      }
      for (k, c) in chunk.enumerate() {
        place(top + left,
          dx: x0 + calc.rem(k, cols) * (card-width + gap),
          dy: y0 + calc.quo(k, cols) * (card-height + gap),
          card(c))
      }
      let small = text.with(font: "Libertinus Serif", size: 5pt)
      if foot >= side {
        place(top + left, dx: x0, dy: y0 + h + marks + 0.8mm, block(width: w, small(notice)))
      } else {
        place(top + left, dx: x0 + w + marks + 0.8mm, dy: y0,
          rotate(90deg, reflow: true, block(width: h, small(notice))))
      }
    }
  }
}
