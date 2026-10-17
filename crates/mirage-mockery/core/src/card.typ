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

#let card-width = 63mm
#let card-height = 88mm

#let sym(path, size) = box(baseline: 14%, image(path, height: size))

#let paragraph(runs) = {
  for r in runs {
    if "symbol" in r { sym(r.symbol, 0.9em) } else if r.reminder { emph(r.text) } else { r.text }
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

#let card(c) = {
  let color = palette.at(c.frame)
  let pale = color.lighten(55%)
  let bar(body) = block(
    width: 100%, fill: pale, stroke: 0.4pt + black, radius: 1mm,
    inset: (x: 1.5mm, y: 1.1mm), body,
  )
  block(width: card-width, height: card-height, fill: black, inset: 2.6mm, radius: 2.5mm, breakable: false,
    block(width: 100%, height: 100%, fill: gradient.linear(color.lighten(15%), color, angle: 90deg),
      radius: 1.2mm, inset: (x: 1.5mm, top: 1.5mm, bottom: 0.6mm), {
      set text(font: "Libertinus Serif", size: 8.5pt, fill: black)
      set block(spacing: 0.9mm)

      // The name gives way to the cost, never the other way round.
      bar(grid(columns: (1fr, auto), align: horizon,
        fit-line(c.name, 9.5pt, 6pt, "the name"),
        { for s in c.cost { sym(s, 9pt) } },
      ))

      block(width: 100%, height: 36.5mm, stroke: 0.4pt + black, clip: true,
        if c.art != none {
          image(c.art, width: 100%, height: 100%, fit: "cover")
        } else {
          rect(width: 100%, height: 100%, stroke: none,
            fill: gradient.linear(color.darken(35%), color.lighten(10%), angle: 60deg))
        })

      bar(fit-line(c.type_line, 8.5pt, 5.5pt, "the type line"))

      let corner = c.corner
      block(width: 100%, height: 1fr, fill: rgb("#f5f0e6"), stroke: 0.4pt + black,
        inset: (x: 2mm, top: 1.8mm, bottom: if corner != none { 5.4mm } else { 1.8mm }),
        layout(size => {
          let body(sz) = {
            set text(size: sz)
            set par(leading: 0.42em, spacing: 0.75em, justify: false)
            for p in c.rules { par(paragraph(p)) }
            if c.flavor != none {
              if c.rules.len() > 0 { line(length: 100%, stroke: 0.3pt) }
              text(style: "italic", c.flavor)
            }
          }
          // Printed cards shrink their type to fit the box rather than spill
          // out of it, so this does too, a quarter point at a time.
          let floor = 5pt
          let fits(sz) = measure(block(width: size.width, body(sz))).height <= size.height
          let sz = 9pt
          while sz > floor and not fits(sz) { sz -= 0.25pt }
          if not fits(sz) { panic("the rules text does not fit its box even at " + repr(floor)) }
          body(sz)
        }))

      if corner != none {
        // On the text box's bottom edge and clear of the footer, with the box's bottom inset
        // keeping the text clear of it.
        place(bottom + right, dx: 0.6mm, dy: 0.2mm,
          box(fill: pale, stroke: 0.4pt + black, radius: 1mm, inset: (x: 2mm, y: 1mm),
            text(weight: "bold", size: 10pt, corner)))
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
