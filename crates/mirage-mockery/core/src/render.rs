//! Cards to PDF and PNG through Typst, compiled in-process.
//!
//! The Typst world here is closed: the template, the job as JSON, the symbols
//! and the art, all handed over up front. Nothing it compiles can read the
//! disk or the network.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
pub use typst_layout::PagedDocument;

use crate::card::{is_fuse, Card, Color, Colors, Corner, Face, Frame};
use crate::placeholder;
use crate::rules::{ability_word, numeral, Ability, LoyaltyCost, Rules};
use crate::symbols::{self, file_name, runs, Run};

const TEMPLATE: &str = include_str!("card.typ");

/// What a run lays out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
    /// Each card a page of its own, trimmed to the card.
    Cards,
    /// As many cards as fit a page, to print and cut.
    Sheet(Sheet),
}

/// The same parameters as Phyrexian Metamorph's proxy sheet, so the two print
/// alike. The sheet is Typst's rather than metamorph-sheet's because that one
/// embeds JPEGs, and a custom card rasterised into one prints soft text in a
/// file twelve times the size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sheet {
    pub paper: Paper,
    gap_mm: f32,
    margin_mm: f32,
    pub cut_marks: bool,
}

impl Sheet {
    pub fn new(paper: Paper, gap_mm: f32, margin_mm: f32, cut_marks: bool) -> Result<Self> {
        if !(gap_mm >= 0.0 && margin_mm >= 0.0) {
            bail!("the gap and the margin are millimetres, none of them negative");
        }
        Ok(Sheet {
            paper,
            gap_mm,
            margin_mm,
            cut_marks,
        })
    }
}

impl Default for Sheet {
    /// Metamorph's defaults: A4, no gap, a 5 mm margin, cut marks.
    fn default() -> Self {
        Sheet {
            paper: Paper::A4,
            gap_mm: 0.0,
            margin_mm: 5.0,
            cut_marks: true,
        }
    }
}

/// Portrait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    A4,
    A3,
    Letter,
    Legal,
}

impl Paper {
    /// Width and height in millimetres.
    pub fn size_mm(self) -> (f32, f32) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::A3 => (297.0, 420.0),
            Paper::Letter => (215.9, 279.4),
            Paper::Legal => (215.9, 355.6),
        }
    }
}

static FONTS: LazyLock<(LazyHash<FontBook>, Vec<Font>)> = LazyLock::new(|| {
    let fonts: Vec<Font> = typst_assets::fonts()
        .flat_map(|data| Font::iter(Bytes::new(data)))
        .collect();
    (LazyHash::new(FontBook::from_fonts(&fonts)), fonts)
});

static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

struct Job {
    main: FileId,
    source: Source,
    files: HashMap<FileId, Bytes>,
}

fn id(path: &str) -> FileId {
    RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("paths here are written in this file"),
    )
    .intern()
}

impl World for Job {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &FONTS.0
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Err(FileError::NotSource)
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.get().vpath().get_without_slash().into()))
    }
    fn font(&self, index: usize) -> Option<Font> {
        FONTS.1.get(index).cloned()
    }
    fn today(&self, _: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// Lay the cards out, reading each card's art from disk. A symbol with no SVG
/// is an error naming it.
pub fn compile(cards: &[Card], layout: Layout) -> Result<PagedDocument> {
    compile_with_art(cards, layout, |path| Ok(Some(std::fs::read(path)?)))
}

/// [`compile`], with `art` reading a card's art: the browser has no disk, and
/// hands over the bytes it fetched from the repo. `None` draws the face
/// without its art, as a preview does while the repo lacks it.
///
/// A double-faced card is two card images, front then back, each a page of
/// its own or a slot on a sheet; a split card is one.
pub fn compile_with_art(
    cards: &[Card],
    layout: Layout,
    art_of: impl FnMut(&Path) -> Result<Option<Vec<u8>>>,
) -> Result<PagedDocument> {
    let mut files = Files {
        files: HashMap::new(),
        arts: 0,
        art_of,
    };
    let mut out = Vec::new();
    for card in cards {
        match card {
            Card::Single(face) => out.push(files.face(face, Extras::default())?),
            Card::Transform { front, back } => {
                // The front carries the back's power and toughness in grey
                // above its own corner, as reminder (CR 712.2c).
                let back_pt = match &back.corner {
                    Corner::PowerToughness { power, toughness } => {
                        json!(format!("{power}/{toughness}"))
                    }
                    _ => Value::Null,
                };
                out.push(files.face(
                    front,
                    Extras {
                        marker: "transform-front",
                        back_pt,
                        ..Extras::default()
                    },
                )?);
                out.push(files.face(
                    back,
                    Extras {
                        marker: "transform-back",
                        ..Extras::default()
                    },
                )?);
            }
            Card::Modal { front, back } => {
                // Each face's hint bar says what the other face is
                // (CR 712.3c).
                let to_front = files.hint(front)?;
                let to_back = files.hint(back)?;
                out.push(files.face(
                    front,
                    Extras {
                        marker: "modal-front",
                        hint: to_back,
                        ..Extras::default()
                    },
                )?);
                out.push(files.face(
                    back,
                    Extras {
                        marker: "modal-back",
                        hint: to_front,
                        ..Extras::default()
                    },
                )?);
            }
            Card::Adventure { main, spell } | Card::Prepare { main, spell } => {
                // One card image: the spell is drawn in the card's text box,
                // on the left page of a book for an Adventure (CR 715.2) and
                // in a frame inset on the right for a prepare spell
                // (CR 722.2).
                let spell = files.face(
                    spell,
                    Extras {
                        inset_spell: true,
                        ..Extras::default()
                    },
                )?;
                out.push(files.face(
                    main,
                    Extras {
                        inset: json!({
                            "kind": if matches!(card, Card::Adventure { .. }) { "adventure" } else { "prepare" },
                            "spell": spell,
                        }),
                        ..Extras::default()
                    },
                )?);
            }
            Card::Split { left, right } => {
                let fuse = card.fuse();
                let half = |files: &mut Files<_>, f| {
                    files.face(
                        f,
                        Extras {
                            drop_fuse: fuse.is_some(),
                            ..Extras::default()
                        },
                    )
                };
                let halves = [half(&mut files, left)?, half(&mut files, right)?];
                let artists: Vec<&str> = card
                    .faces()
                    .iter()
                    .filter_map(|f| f.artist.as_deref())
                    .collect();
                out.push(json!({
                    "kind": "split",
                    "halves": halves,
                    "fuse": match &fuse {
                        Some(line) => files.paragraph(line).with_context(|| format!("in {:?}'s fuse", card.name()))?,
                        None => Value::Null,
                    },
                    "artist": if artists.is_empty() { Value::Null } else {
                        let mut a = artists;
                        a.dedup();
                        json!(a.join(" & "))
                    },
                }));
            }
        }
    }
    let mut files = files.files;
    let job = json!({
        "layout": match layout { Layout::Cards => "card", Layout::Sheet(_) => "sheet" },
        "sheet": match layout {
            Layout::Cards => Value::Null,
            Layout::Sheet(sheet) => json!({
                "width_mm": sheet.paper.size_mm().0,
                "height_mm": sheet.paper.size_mm().1,
                "gap_mm": sheet.gap_mm,
                "margin_mm": sheet.margin_mm,
                "cut_marks": sheet.cut_marks,
            }),
        },
        "cards": out,
    });
    files.insert(id("/job.json"), Bytes::new(serde_json::to_vec(&job)?));
    let main = id("/main.typ");
    let world = Job {
        main,
        source: Source::new(main, TEMPLATE.into()),
        files,
    };
    let compiled = typst::compile::<PagedDocument>(&world);
    compiled.output.map_err(|errors| explain(&errors))
}

/// What a face of a two-faced card prints beyond itself.
#[derive(Default)]
struct Extras {
    /// Which face it is, on the name bar's left: `transform-front`,
    /// `transform-back`, `modal-front` or `modal-back`; empty for none.
    marker: &'static str,
    /// A transforming front's reminder of its back's power and toughness.
    back_pt: Value,
    /// A modal face's hint bar about the other face.
    hint: Value,
    /// A fused split card prints its Fuse line once across both halves.
    drop_fuse: bool,
    /// The spell inset in this face's text box, an Adventure or a prepare
    /// spell, with which it is.
    inset: Value,
    /// This face is that spell: drawn in another's text box, with no art.
    inset_spell: bool,
}

/// The files a job hands Typst: each symbol once, and each face's art.
struct Files<F> {
    files: HashMap<FileId, Bytes>,
    arts: usize,
    art_of: F,
}

impl<F: FnMut(&Path) -> Result<Option<Vec<u8>>>> Files<F> {
    fn symbol(&mut self, code: &str) -> Result<Value> {
        let Some(svg) = symbols::svg(code) else {
            bail!("no symbol {{{code}}}");
        };
        let path = format!("/symbols/{}", file_name(code));
        self.files
            .entry(id(&path))
            .or_insert_with(|| Bytes::new(svg));
        Ok(json!(path))
    }

    fn paragraph(&mut self, line: &str) -> Result<Value> {
        paragraph(line, &mut |code: &str| self.symbol(code))
    }

    fn cost(&mut self, face: &Face) -> Result<Value> {
        face.cost
            .iter()
            .map(|s| self.symbol(s))
            .collect::<Result<Vec<_>>>()
            .map(Value::from)
            .with_context(|| format!("in {:?}'s cost", face.name))
    }

    /// What a modal face's hint bar says of `other`: its subtypes, or its
    /// card types when it has none, and its cost or, for a land with none,
    /// the mana it adds (CR 712.3c). Printed hints say "Elephant" for a
    /// creature and "Land" for a land, so this does too.
    fn hint(&mut self, other: &Face) -> Result<Value> {
        let (types, subtypes) = other
            .type_line
            .split_once('—')
            .unwrap_or((&other.type_line, ""));
        let types = match subtypes.trim() {
            "" => types
                .split_whitespace()
                .filter(|w| !matches!(*w, "Legendary" | "Basic" | "Snow" | "World"))
                .collect::<Vec<_>>()
                .join(" "),
            subtypes => subtypes.to_string(),
        };
        let land = other.type_line.split_whitespace().any(|w| w == "Land");
        let mana = match other.mana_ability() {
            Some(m) if land && other.cost.is_empty() => self.paragraph(&m)?,
            _ => Value::Null,
        };
        Ok(json!({
            "type": curl(&types),
            "cost": self.cost(other)?,
            "mana": mana,
        }))
    }

    /// The face's art as a file of the job, or a placeholder drawn for it.
    fn art(&mut self, face: &Face) -> Result<Value> {
        let bytes = match &face.art {
            None => None,
            Some(path) => (self.art_of)(path)
                .with_context(|| format!("reading {:?}'s art {}", face.name, path.display()))?
                .map(|b| (path, b)),
        };
        // A face with no art, or whose art the browser does not have yet,
        // prints a placeholder drawn for it.
        let (ext, bytes) = match bytes {
            Some((path, bytes)) => (
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png")
                    .to_string(),
                bytes,
            ),
            None => {
                let frame = face.frame();
                let colors = match frame {
                    Frame::Land(made) => made.ordered(),
                    _ => face.colors().ordered(),
                };
                (
                    "svg".to_string(),
                    placeholder::art(&face.name, frame, &colors)?.into_bytes(),
                )
            }
        };
        let virt = format!("/art/{}.{ext}", self.arts);
        self.arts += 1;
        self.files.insert(id(&virt), Bytes::new(bytes));
        Ok(json!(virt))
    }

    fn face(&mut self, face: &Face, extras: Extras) -> Result<Value> {
        let cost = self.cost(face)?;
        let mut set = |lines: &[String]| -> Result<Value> {
            lines
                .iter()
                .filter(|l| !(extras.drop_fuse && is_fuse(l)))
                .map(|l| self.paragraph(l))
                .collect::<Result<Vec<_>>>()
                .map(Value::from)
                .with_context(|| format!("in {:?}'s rules text", face.name))
        };
        let rules = match &face.rules {
            Rules::Plain(lines) => json!({ "kind": "plain", "lines": set(lines)? }),
            Rules::Saga(saga) => json!({
                "kind": "saga",
                "before": set(&saga.before)?,
                "chapters": saga.chapters().map(|(span, text)| Ok(json!({
                    "numerals": span.map(numeral).collect::<Vec<_>>(),
                    "text": set(&[text.to_string()])?,
                }))).collect::<Result<Vec<_>>>()?,
                "after": set(&saga.after)?,
            }),
            Rules::Class(class) => json!({
                "kind": "class",
                "before": set(&class.before)?,
                "levels": class.levels().map(|(cost, n, text)| Ok(json!({
                    "cost": set(&[cost.to_string()])?,
                    "level": n,
                    "text": set(text)?,
                }))).collect::<Result<Vec<_>>>()?,
            }),
            Rules::Planeswalker(walker) => json!({
                "kind": "planeswalker",
                "abilities": walker.abilities().iter().map(|a| Ok(match a {
                    Ability::Static(line) => json!({ "loyalty": Value::Null, "text": set(std::slice::from_ref(line))? }),
                    Ability::Loyalty(cost, text) => json!({
                        "loyalty": {
                            "cost": cost.to_string(),
                            "sign": match cost {
                                LoyaltyCost::Plus(_) => "plus",
                                LoyaltyCost::Minus(_) => "minus",
                                LoyaltyCost::Zero => "zero",
                            },
                        },
                        "text": set(std::slice::from_ref(text))?,
                    }),
                })).collect::<Result<Vec<_>>>()?,
            }),
            Rules::Station(station) => json!({
                "kind": "station",
                "before": set(&station.before)?,
                "thresholds": station.thresholds().map(|(at, text)| Ok(json!({
                    "at": format!("{at}+"),
                    "text": set(text)?,
                }))).collect::<Result<Vec<_>>>()?,
            }),
        };
        // The inset spell is drawn in another face's text box, with none.
        let art = if extras.inset_spell {
            Value::Null
        } else {
            self.art(face)?
        };
        Ok(json!({
            // A battle is drawn on its side, its type line says so (CR 310).
            "kind": if face.is_battle() { "battle" } else { "face" },
            "name": curl(&face.name),
            "cost": cost,
            "type_line": curl(&face.type_line),
            "indicator": names(face.color_indicator),
            "rules": rules,
            "flavor": face.flavor.as_deref().map(curl),
            "corner": match &face.corner {
                Corner::None => Value::Null,
                Corner::PowerToughness { power, toughness } => json!(format!("{power}/{toughness}")),
                Corner::Loyalty(l) => json!(l),
                Corner::Defense(d) => json!(d),
            },
            "corner_kind": match &face.corner {
                Corner::None => Value::Null,
                Corner::PowerToughness { .. } => json!("pt"),
                Corner::Loyalty(_) => json!("loyalty"),
                Corner::Defense(_) => json!("defense"),
            },
            "frame": match face.frame() {
                Frame::Mono(c) => json!({ "base": color_name(c), "tint": [] }),
                // A gold card's bars are rimmed in its colours, as printed ones are.
                Frame::Gold => json!({ "base": "gold", "tint": [], "colors": names(face.colors()) }),
                Frame::Artifact => json!({ "base": "artifact", "tint": [] }),
                Frame::Colorless => json!({ "base": "colorless", "tint": [] }),
                Frame::Land(colors) => json!({ "base": "land", "tint": names(colors) }),
            },
            "art": art,
            "artist": face.artist,
            "marker": if extras.marker.is_empty() { Value::Null } else { json!(extras.marker) },
            "back_pt": extras.back_pt,
            "hint": extras.hint,
            "inset": extras.inset,
        }))
    }
}

/// A paragraph as the runs the template sets, its quotes curled first so a
/// quote beside a symbol still knows which way it faces, and an ability or
/// flavor word opening it marked to be set in italics.
fn paragraph(line: &str, symbol: &mut impl FnMut(&str) -> Result<Value>) -> Result<Value> {
    let line = curl(line);
    let (word, rest) = match ability_word(&line) {
        Some((word, rest)) => (Some(word), rest),
        None => (None, line.as_str()),
    };
    word.map(|w| Ok(json!({ "text": w, "reminder": false, "word": true })))
        .into_iter()
        .chain(runs(rest).into_iter().map(|run| {
            Ok(match run {
                Run::Words { text, reminder } => json!({ "text": text, "reminder": reminder }),
                Run::Symbol(code) => json!({ "symbol": symbol(&code)? }),
            })
        }))
        .collect::<Result<Vec<_>>>()
        .map(Value::from)
}

fn color_name(c: Color) -> &'static str {
    match c {
        Color::White => "white",
        Color::Blue => "blue",
        Color::Black => "black",
        Color::Red => "red",
        Color::Green => "green",
    }
}

fn names(colors: Colors) -> Vec<&'static str> {
    colors.ordered().into_iter().map(color_name).collect()
}

/// Straight quotes as printed cards set them: `don't` as `don’t`, and
/// `"…"` as `“…”`. A quote opens after a space, a bracket, a dash or another
/// opening quote, and closes (or is an apostrophe) anywhere else. The file
/// keeps what was typed; only the print is curled.
pub fn curl(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut before: Option<char> = None;
    for c in text.chars() {
        let opens = before.is_none_or(|b| b.is_whitespace() || "([{—–-“‘".contains(b));
        let set = match c {
            '\'' if opens => '‘',
            '\'' => '’',
            '"' if opens => '“',
            '"' => '”',
            c => c,
        };
        out.push(set);
        before = Some(set);
    }
    out
}

fn explain<'a>(errors: impl IntoIterator<Item = &'a SourceDiagnostic>) -> anyhow::Error {
    let lines: Vec<String> = errors
        .into_iter()
        .map(|e| {
            let hints = e.hints.iter().map(|h| format!(" (hint: {})", h.v));
            format!("{}{}", e.message, hints.collect::<String>())
        })
        .collect();
    anyhow!("typst: {}", lines.join("; "))
}

pub fn pdf(doc: &PagedDocument) -> Result<Vec<u8>> {
    typst_pdf::pdf(doc, &typst_pdf::PdfOptions::default()).map_err(|e| explain(&e))
}

/// One page as a PNG at `ppi` pixels per inch.
pub fn png(doc: &PagedDocument, page: usize, ppi: f32) -> Result<Vec<u8>> {
    let Some(page) = doc.pages().get(page) else {
        bail!("no page {page}");
    };
    let pixmap = typst_render::render(
        page,
        &typst_render::RenderOptions {
            pixel_per_pt: (ppi as f64 / 72.0).into(),
            render_bleed: false,
        },
    );
    pixmap
        .encode_png()
        .map_err(|e| anyhow!("encoding png: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{curl, paragraph};
    use serde_json::json;

    #[test]
    fn an_ability_word_is_its_own_run_set_in_italics() {
        let mut no_symbol = |_: &str| unreachable!();
        let runs = paragraph(
            "Landfall — Whenever a land you control enters, draw a card.",
            &mut no_symbol,
        )
        .unwrap();
        assert_eq!(
            runs[0],
            json!({ "text": "Landfall", "reminder": false, "word": true })
        );
        assert_eq!(
            runs[1]["text"],
            " — Whenever a land you control enters, draw a card."
        );
        let runs = paragraph("Flying", &mut no_symbol).unwrap();
        assert_eq!(runs.as_array().unwrap().len(), 1);
        assert!(runs[0].get("word").is_none());
    }

    #[test]
    fn quotes_curl_the_way_they_face() {
        assert_eq!(curl("you don't control"), "you don’t control");
        assert_eq!(curl("it has \"{T}: Add {G}.\""), "it has “{T}: Add {G}.”");
        assert_eq!(
            curl("\"Say 'no' twice.\" —Ajani's rule"),
            "“Say ‘no’ twice.” —Ajani’s rule"
        );
    }
}
