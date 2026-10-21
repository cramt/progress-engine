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
    if "symbol" in r { sym(r.symbol, 0.9em) } else if r.reminder { emph(r.text) } else { r.text }
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

#let corner-box(c, ps) = box(fill: ps.bar, stroke: 0.4pt + black, radius: 1mm, inset: (x: 2mm, y: 1mm),
  text(weight: "bold", size: 10pt, c.corner))

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
  block(width: 100%, height: 1fr, fill: ps.box, stroke: 0.4pt + black, clip: false,
    inset: (x: pad, top: 1.8mm, bottom: if c.corner != none and not corner-in-band { 5.4mm } else { 1.8mm }),
    fit-box((sz, spare) => text-style(sz, {
      if r.kind == "plain" { paragraphs(r.lines) } else {
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
      for ch in sections {
        fill-to(h => block(width: 100%, height: h, stroke: (top: 0.3pt + black),
          grid(columns: (5.4mm, 1fr), rows: (if h == auto { auto } else { h },),
            block(width: 100%, height: if h == auto { auto } else { 100% }, fill: ps.bar,
              stroke: (right: 0.3pt + black), inset: (y: 1mm),
              align(center + horizon, stack(spacing: 0.6mm, ..ch.numerals.map(n => chapter-mark(n, ps))))),
            block(inset: pad, height: if h == auto { auto } else { 100% },
              align(horizon, paragraphs(ch.text))))))
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

#let card(c) = {
  let ps = paints(c.frame)
  let bar(body) = block(
    width: 100%, fill: ps.bar, stroke: 0.4pt + black, radius: 1mm,
    inset: (x: 1.5mm, y: 1.1mm), body,
  )
  let type-bar(room: 0mm) = bar(pad(right: room, if c.indicator.len() > 0 {
    grid(columns: (auto, 1fr), column-gutter: 1.2mm, align: horizon,
      indicator(c.indicator), fit-line(c.type_line, 8.5pt, 5.5pt, "the type line"))
  } else { fit-line(c.type_line, 8.5pt, 5.5pt, "the type line") }))
  block(width: card-width, height: card-height, fill: black, inset: 2.6mm, radius: 2.5mm, breakable: false,
    block(width: 100%, height: 100%, fill: ps.body,
      radius: 1.2mm, inset: (x: 1.5mm, top: 1.5mm, bottom: 0.6mm), {
      set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
      set block(spacing: 0.9mm)

      // The name gives way to the cost, never the other way round.
      bar(grid(columns: (1fr, auto), align: horizon,
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
        type-bar(room: if c.corner != none and not boxed { 11mm } else { 0mm })
        if boxed {
          block(width: 100%, height: 9mm, fill: ps.box, stroke: 0.4pt + black,
            inset: (left: 2mm, right: if c.corner != none { 12mm } else { 2mm }, y: 1.2mm),
            fit-box((sz, _) => text-style(sz, align(center + horizon, paragraphs(after))),
              what: "the text below the type line"))
        }
        v(4.6mm)
        if c.corner != none {
          place(bottom + right, dx: 0.6mm, dy: if boxed { -2.2mm } else { -4.2mm }, corner-box(c, ps))
        }
      } else {
        art-box(c, ps, height: 36.5mm)
        type-bar()
        standard-text(c, ps)
        if c.corner != none and not (r.kind == "station" and r.thresholds.len() > 0) {
          // On the text box's bottom edge and clear of the footer, with the box's bottom inset
          // keeping the text clear of it.
          place(bottom + right, dx: 0.6mm, dy: 0.2mm, corner-box(c, ps))
        }
      }

      // Where a real card's legal line sits, this says what it is.
      set text(size: 4.2pt, fill: white)
      place(bottom + left, dy: 2.2mm,
        if c.artist != none [Illus. #c.artist] else [])
      place(bottom + right, dy: 2.2mm, [Unofficial fan content · not a Magic card])
    }))
}

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
