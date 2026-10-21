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
#let paints(frame) = {
  let base = palette.at(frame.base)
  let tint = frame.tint.map(t => palette.at(t))
  if frame.base != "land" or tint.len() == 0 {
    (
      body: gradient.linear(base.lighten(15%), base, angle: 90deg),
      bar: base.lighten(55%),
      box: paper,
      accent: base,
    )
  } else if tint.len() == 1 {
    let c = tint.first()
    let mixed = color.mix((base, 25%), (c, 75%))
    (
      body: gradient.linear(mixed.lighten(15%), mixed, angle: 90deg),
      bar: c.lighten(55%),
      box: c.lighten(68%),
      accent: c,
    )
  } else if tint.len() == 2 {
    // Blended across rather than cut, as a dual land's text box is.
    let mixed = tint.map(c => color.mix((base, 15%), (c, 85%)).darken(8%))
    (
      body: gradient.linear(..mixed),
      bar: base.lighten(55%),
      box: gradient.linear(..tint.map(c => c.lighten(50%))),
      accent: base,
    )
  } else {
    let mixed = color.mix((base, 25%), (palette.gold, 75%))
    (
      body: gradient.linear(mixed.lighten(15%), mixed, angle: 90deg),
      bar: palette.gold.lighten(55%),
      box: palette.gold.lighten(68%),
      accent: palette.gold,
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

// Text set to fill a box: printed cards shrink their type to fit rather than
// spill, so this does too, to the quarter point. `body(sz, spare)` is the
// text at a size, handed the height left over so sections can share it out.
// The largest quarter point that fits is found by halving, not by stepping
// down: every try lays the text out, and a Saga's sections each twice.
#let fit-box(body, what: "the rules text") = layout(size => {
  let floor = 5pt
  let height(sz) = measure(block(width: size.width, body(sz, 0pt))).height
  let fits(q) = height(floor + q * 0.25pt) <= size.height
  // Quarter points above the floor: 0 is 5pt, 16 is 9pt.
  let (lo, hi) = (0, 16)
  if not fits(lo) { panic(what + " does not fit its box even at " + repr(floor)) }
  if fits(hi) { lo = hi }
  while hi - lo > 1 {
    let mid = calc.quo(lo + hi, 2)
    if fits(mid) { lo = mid } else { hi = mid }
  }
  let sz = floor + lo * 0.25pt
  body(sz, size.height - height(sz))
})

#let text-style(sz, body) = {
  set text(size: sz)
  set par(leading: 0.42em, spacing: 0.75em, justify: false)
  body
}

#let paragraphs(lines) = for p in lines { par(paragraph(p)) }

#let flavor-text(c) = text(style: "italic", c.flavor)

// A loyalty cost's badge (CR 606.4), Mockery's own: a dark plate whose top
// rises to a point for a cost that adds counters, whose foot drops to one for
// a cost that removes them, and flat for neither.
#let loyalty-badge(cost, sign) = {
  let (w, h, tip) = (7mm, 4.2mm, 1mm)
  let pts = if sign == "plus" {
    ((0mm, tip), (w / 2, 0mm), (w, tip), (w, h), (0mm, h))
  } else if sign == "minus" {
    ((0mm, 0mm), (w, 0mm), (w, h - tip), (w / 2, h), (0mm, h - tip))
  } else {
    ((0.6mm, 0mm), (w - 0.6mm, 0mm), (w, h / 2), (w - 0.6mm, h), (0.6mm, h), (0mm, h / 2))
  }
  box(width: w, height: h, {
    place(polygon(fill: luma(28), stroke: 0.5pt + white, ..pts))
    place(center + horizon, dy: if sign == "plus" { 0.3mm } else if sign == "minus" { -0.3mm } else { 0mm },
      text(size: 7.4pt, weight: "bold", fill: white, cost))
  })
}

// The corner: power and toughness or defense on a bar-coloured plate, and a
// planeswalker's loyalty on a dark shield, flat at the top and pointed at the
// foot, so loyalty never reads as a power and toughness.
#let corner-box(c, ps) = if c.corner_kind == "loyalty" {
  let (w, h) = (9mm, 5.6mm)
  box(width: w, height: h, {
    place(polygon(fill: luma(28), stroke: 0.5pt + white,
      (0mm, 0mm), (w, 0mm), (w, h - 1.4mm), (w / 2, h), (0mm, h - 1.4mm)))
    place(center + horizon, dy: -0.5mm, text(weight: "bold", size: 10pt, fill: white, c.corner))
  })
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
#let standard-text(c, ps) = {
  let r = c.rules
  let pad = 2mm
  let striated = r.kind == "station" and r.thresholds.len() > 0
  let corner-in-band = striated and c.corner != none
  // A planeswalker's last band makes room for the loyalty beside its text,
  // as printed ones do, rather than the whole box stopping above it.
  let walker-corner = (r.kind == "planeswalker" and c.flavor == none and c.hint == none
    and c.back_pt == none and r.abilities.len() > 0)
  let bottom = if walker-corner { 1.8mm } else if (c.corner != none and not corner-in-band) or c.hint != none or c.back_pt != none { 5.4mm } else { 1.8mm }
  block(width: 100%, height: 1fr, fill: ps.box, stroke: 0.4pt + black, clip: false,
    inset: (x: pad, top: 1.8mm, bottom: bottom),
    fit-box((sz, spare) => text-style(sz, {
      if r.kind == "plain" { paragraphs(r.lines) } else if r.kind == "planeswalker" {
        // Static abilities across the box; each loyalty ability a band of
        // its own, shaded in turn, its badge straddling the box's left edge
        // and the bands sharing the height left over, as printed
        // planeswalkers' text boxes are (CR 606).
        let bands = r.abilities.filter(a => a.loyalty != none).len()
        let share = spare / calc.max(bands, 1)
        let (badge, gap) = (7mm, 1.5mm)
        let indent = badge / 2 + gap - pad
        let n = 0
        let last = r.abilities.len() - 1
        for (i, a) in r.abilities.enumerate() {
          let room = if i == last and walker-corner and c.corner != none { 9.5mm - pad } else { 0mm }
          if a.loyalty == none { block(above: 1.2mm, below: 1.2mm, inset: (right: room), paragraphs(a.text)) } else {
            let shade = if calc.rem(n, 2) == 0 { black.transparentize(93%) } else { white.transparentize(55%) }
            n += 1
            let band(h) = block(width: 100%, height: h, breakable: false, fill: shade,
              // The first and last bands run to the box's edges.
              outset: (x: pad, top: if i == 0 { 1.8mm } else { 0mm },
                bottom: if i == last and c.flavor == none { bottom } else { 0mm }),
              above: 0mm, below: 0mm,
              inset: (left: indent, right: room, y: 0.8mm), {
                place(left + horizon, dx: -indent - pad - badge / 2, loyalty-badge(a.loyalty.cost, a.loyalty.sign))
                place(left + horizon, dx: -gap + 0.35mm, text(weight: "bold", [:]))
                align(horizon, paragraphs(a.text))
              })
            layout(size => band(measure(band(auto), width: size.width).height + share))
          }
        }
      } else {
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
        if r.kind != "plain" or r.lines.len() > 0 { line(length: 100%, stroke: 0.3pt) }
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
  fit-box((sz, spare) => text-style(sz, {
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
// and two on the back.
#let face-marker(kind) = {
  let chevron(dx, dy, up: true, w: 1.1mm, h: 0.65mm, paint: white) = place(top + left, dx: dx, dy: dy,
    polygon(fill: paint, stroke: none,
      ..if up { ((0mm, h), (w / 2, 0mm), (w, h), (w, h + 0.45mm), (w / 2, 0.45mm), (0mm, h + 0.45mm)) }
        else { ((0mm, 0mm), (w / 2, h), (w, 0mm), (w, 0.45mm), (w / 2, h + 0.45mm), (0mm, 0.45mm)) }))
  if kind == "transform-front" or kind == "transform-back" {
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
#let hint-bar(hint, front) = {
  let (bg, fg) = if front { (luma(35), white) } else { (luma(235), black) }
  box(fill: bg, stroke: 0.4pt + black, radius: (right: 1.4mm),
    inset: (left: 1mm, right: 1.6mm, y: 0.7mm), {
    set text(size: 5.2pt, fill: fg)
    stack(dir: ltr, spacing: 1.2mm,
      box(baseline: -0.2em, polygon(fill: fg, stroke: none, (0.9mm, 0mm), (0mm, 0.6mm), (0.9mm, 1.2mm))),
      text(weight: "bold", hint.type),
      h(0.6mm),
      if hint.mana != none { paragraph(hint.mana) } else { for s in hint.cost { sym(s, 5.4pt) } })
  })
}


// One face's body, filling the box it is given: a card's whole face, or a
// split card's half with `art` a share of the half rather than a length.
// A fused half's name and type bars `point` at the other half, as printed
// fuse cards' do.
#let face-body(c, art: 36.5mm, point: none) = {
  let ps = paints(c.frame)
  set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
  set block(spacing: 0.9mm)
  let bar(body) = if point == none {
    block(width: 100%, fill: ps.bar, stroke: 0.4pt + black, radius: 1mm,
      inset: (x: 1.5mm, y: 1.1mm), body)
  } else {
    // The slant starts `a` in from the end and its tip stands `out` past it,
    // over the gutter between the halves.
    let (a, out) = (1.6mm, 1.5mm)
    layout(size => {
      let inner = block(width: 100%, body,
        inset: (y: 1.1mm, left: if point == "left" { 1.5mm + a } else { 1.5mm },
          right: if point == "right" { 1.5mm + a } else { 1.5mm }))
      let (w, h) = (size.width, measure(block(width: size.width, inner)).height)
      let ends = if point == "right" {
        ((0mm, 0mm), (w - a, 0mm), (w + out, h / 2), (w - a, h), (0mm, h))
      } else { ((w, 0mm), (a, 0mm), (-out, h / 2), (a, h), (w, h)) }
      block(width: 100%, { place(polygon(fill: ps.bar, stroke: 0.4pt + black, ..ends)); inner })
    })
  }
  let type-bar(room: 0mm) = bar(pad(right: room, if c.indicator.len() > 0 {
    grid(columns: (auto, 1fr), column-gutter: 1.2mm, align: horizon,
      indicator(c.indicator), fit-line(c.type_line, 8.5pt, 5.5pt, "the type line"))
  } else { fit-line(c.type_line, 8.5pt, 5.5pt, "the type line") }))
  // The corner, with a transforming front's reminder of its back's beside it.
  let foot-right = if c.corner != none and c.back_pt != none {
    stack(dir: ltr, spacing: 1mm, move(dy: -1.6mm, back-pt(c)), corner-box(c, ps))
  } else if c.corner != none { corner-box(c, ps) } else if c.back_pt != none {
    move(dy: -1.3mm, back-pt(c))
  } else { none }

  // The name gives way to the cost, never the other way round.
  bar(grid(columns: if c.marker != none { (auto, 1fr, auto) } else { (1fr, auto) },
    column-gutter: 1mm, align: horizon,
    ..if c.marker != none { (face-marker(c.marker),) },
    fit-line(c.name, 9.5pt, 6pt, "the name"),
    { for s in c.cost { sym(s, 9pt) } },
  ))

  let r = c.rules
  if r.kind == "saga" or r.kind == "class" {
    // The art stands down one side and the type line goes to the foot:
    // on the right of a Saga, the left of a Class (CR 714.1, 716.1).
    let text-col = column-text(c, ps)
    let art-col = art-box(c, ps)
    block(width: 100%, height: 1fr, grid(columns: if r.kind == "saga" { (54%, 1fr) } else { (1fr, 54%) },
      column-gutter: 0.9mm, rows: (100%,),
      ..if r.kind == "saga" { (text-col, art-col) } else { (art-col, text-col) }))
    // A Saga creature's other abilities, and its power and toughness, sit
    // in a text box below the type line (CR 714.1a). With no abilities
    // there the box would be empty, so the corner rides on the type line.
    let after = if r.kind == "saga" { r.after } else { () }
    let boxed = after.len() > 0
    // A transforming Saga's reminder of its back's power and toughness rides
    // on the type line's end, the corner being the back's.
    let reminder = c.back_pt != none and c.corner == none and r.kind != "saga"
    type-bar(room: if c.corner != none and not boxed { 11mm } else if reminder { 7mm } else { 0mm })
    if reminder { place(bottom + right, dx: -1.6mm, dy: -5.9mm, back-pt(c)) }
    if boxed {
      block(width: 100%, height: 9mm, fill: ps.box, stroke: 0.4pt + black,
        inset: (left: 2mm, right: if c.corner != none { 12mm } else { 2mm }, y: 1.2mm),
        fit-box((sz, _) => text-style(sz, align(center + horizon, paragraphs(after))),
          what: "the text below the type line"))
    }
    v(4.6mm)
    if foot-right != none and not (c.back_pt != none and c.corner == none) {
      place(bottom + right, dx: 0.6mm, dy: if boxed { -2.2mm } else if c.corner != none { -4.2mm } else { -1.4mm }, foot-right)
    }
    if c.hint != none { place(bottom + left, dx: -0.6mm, dy: -0.6mm, hint-bar(c.hint, c.marker == "modal-front")) }
  } else {
    art-box(c, ps, height: art)
    type-bar()
    standard-text(c, ps)
    if c.hint != none { place(bottom + left, dx: -0.6mm, dy: -0.2mm, hint-bar(c.hint, c.marker == "modal-front")) }
    if foot-right != none and not (r.kind == "station" and r.thresholds.len() > 0 and c.back_pt == none) {
      // On the text box's bottom edge and clear of the footer, with the box's bottom inset
      // keeping the text clear of it.
      let in-band = r.kind == "station" and r.thresholds.len() > 0
      place(bottom + right, dx: 0.6mm, dy: 0.2mm,
        if in-band { move(dy: -1.3mm, back-pt(c)) } else { foot-right })
    }
  }
}

// Where a real card's legal line sits, this says what it is.
#let footer(artist) = {
  set text(font: "Libertinus Serif", size: 4.2pt, fill: white)
  place(bottom + left, dy: 2.2mm, if artist != none [Illus. #artist] else [])
  place(bottom + right, dy: 2.2mm, [Unofficial fan content · not a Magic card])
}

#let face-card(c) = block(width: card-width, height: card-height, fill: black, inset: 2.6mm, radius: 2.5mm, breakable: false,
  block(width: 100%, height: 100%, fill: paints(c.frame).body,
    radius: 1.2mm, inset: (x: 1.5mm, top: 1.5mm, bottom: 0.6mm), {
    face-body(c)
    footer(c.artist)
  }))

// A split card: two halves side by side on a card turned on its side, read
// with the card's left edge at the bottom (CR 709.1), each half a face of its
// own. Halves that both have fuse print it once, on a bar across the foot of
// both (CR 702.102a), and their name and type bars point at each other.
#let split-card(c) = rotate(-90deg, reflow: true,
  block(width: card-height, height: card-width, fill: black, inset: 2.6mm, radius: 2.5mm, breakable: false, {
    set block(spacing: 0mm)
    block(width: 100%, height: 100%, grid(columns: (1fr, 1fr), column-gutter: 1mm, rows: (1fr, auto), row-gutter: 0.9mm,
      ..c.halves.enumerate().map(((i, h)) => block(width: 100%, height: 100%, fill: paints(h.frame).body,
        radius: 1.2mm, inset: (x: 1.3mm, top: 1.3mm, bottom: 1.3mm),
        face-body(h, art: 38%, point: if c.fuse == none { none } else if i == 0 { "right" } else { "left" }))),
      ..if c.fuse != none {
        let ends = c.halves.map(h => paints(h.frame).bar)
        (grid.cell(colspan: 2, block(width: 100%, fill: gradient.linear(..ends), stroke: 0.4pt + black,
          radius: 1mm, inset: (x: 1.6mm, y: 1mm),
          text(font: "Libertinus Serif", size: 6.6pt, fill: black, paragraph(c.fuse)))),)
      }))
    footer(c.artist)
  }))

#let card(c) = if c.kind == "split" { split-card(c) } else { face-card(c) }

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
