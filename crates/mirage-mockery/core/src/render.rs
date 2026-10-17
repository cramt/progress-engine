//! Cards to PDF and PNG through Typst, compiled in-process.
//!
//! The Typst world here is closed: the template, the job as JSON, the symbols
//! and the art, all handed over up front. Nothing it compiles can read the
//! disk or the network.

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::card::{Card, Corner, Frame};
use crate::symbols::{file_name, runs, Run, Symbols};

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

/// Lay the cards out. Every symbol the cards use must be in `symbols`.
pub fn compile(cards: &[Card], symbols: &Symbols, layout: Layout) -> Result<PagedDocument> {
    let mut files = HashMap::new();
    let mut symbol = |code: &str| -> Result<Value> {
        symbols.require(code)?;
        let path = format!("/symbols/{}", file_name(code));
        files
            .entry(id(&path))
            .or_insert_with(|| Bytes::new(symbols.svgs[code].clone()));
        Ok(json!(path))
    };
    let mut out = Vec::new();
    let mut art = Vec::new();
    for (i, card) in cards.iter().enumerate() {
        let cost = card
            .cost
            .iter()
            .map(|s| symbol(s))
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("in {:?}'s cost", card.name))?;
        let mut rules = Vec::new();
        for paragraph in &card.rules {
            let mut set = Vec::new();
            for run in runs(paragraph) {
                set.push(match run {
                    Run::Words { text, reminder } => json!({ "text": text, "reminder": reminder }),
                    Run::Symbol(code) => json!({
                        "symbol": symbol(&code)
                            .with_context(|| format!("in {:?}'s rules text", card.name))?
                    }),
                });
            }
            rules.push(set);
        }
        let art_path = match &card.art {
            None => Value::Null,
            Some(path) => {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png");
                let virt = format!("/art/{i}.{ext}");
                let bytes = std::fs::read(path)
                    .with_context(|| format!("reading {:?}'s art {}", card.name, path.display()))?;
                art.push((id(&virt), Bytes::new(bytes)));
                json!(virt)
            }
        };
        out.push(json!({
            "name": card.name,
            "cost": cost,
            "type_line": card.type_line,
            "rules": rules,
            "flavor": card.flavor,
            "corner": match &card.corner {
                Corner::None => Value::Null,
                Corner::PowerToughness { power, toughness } => json!(format!("{power}/{toughness}")),
                Corner::Loyalty(l) => json!(l),
                Corner::Defense(d) => json!(d),
            },
            "frame": match card.frame() {
                Frame::White => "white",
                Frame::Blue => "blue",
                Frame::Black => "black",
                Frame::Red => "red",
                Frame::Green => "green",
                Frame::Gold => "gold",
                Frame::Artifact => "artifact",
                Frame::Land => "land",
                Frame::Colorless => "colorless",
            },
            "art": art_path,
            "artist": card.artist,
        }));
    }
    files.extend(art);
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
