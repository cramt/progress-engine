//! Mirage Mockery in the browser: Curator's custom cards page reads, checks
//! and renders a `.card.toml` with the same `mockery-core` the CLI runs.
//!
//! The page does the fetching of the card's art from the repo. Nothing here
//! touches the network.
//! `meldweb-curator/web/src/mockery.gen.ts` is generated from the wire types
//! below; the test at the bottom fails when it is stale.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use facet::Facet;
use mockery_core::card::{Card, Reading};
use mockery_core::form::{Form, Parts, Side};
use mockery_core::read::read;
use mockery_core::render::{self, Layout, Paper, Sheet};
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

/// A line of rules text Insight could not read, and the word it stopped at.
#[derive(Debug, Facet)]
pub struct Unread {
    pub line: String,
    pub read: String,
    pub at: String,
}

/// A `.card.toml`, read.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Checked {
    Card {
        /// Both names of a card of two, `Far // Away`.
        name: String,
        /// The front face's, or the left half's.
        type_line: String,
        /// Every face's art path, relative to the card file, front first.
        arts: Vec<String>,
        /// The card images it prints, in the order [`render_png`] draws
        /// them: one, a double-faced card's two, or a meld pair's four.
        images: Vec<Image>,
        lines: u32,
        unread: Vec<Unread>,
    },
    Refused {
        message: String,
    },
}

/// One card image a file prints: what the preview's picker calls it, and
/// which of the form's parts is drawn on it, so picking one picks the other.
#[derive(Debug, Facet)]
pub struct Image {
    pub label: String,
    /// The form's part: 0 the top level, 1 the second table, 2 a meld
    /// pair's `[melded]` card.
    pub part: u32,
    /// How it is read: the PNG is always the portrait card, and the preview
    /// turns the ones read on their side.
    pub reading: Reading,
}

fn images_of(card: &Card) -> Vec<Image> {
    let images: &[(&str, u32)] = match card {
        Card::Transform { .. } | Card::Modal { .. } => &[("Front", 0), ("Back", 1)],
        // Each card's back is half the melded card.
        Card::Meld { .. } => &[
            ("Front", 0),
            ("Back", 2),
            ("Counterpart", 1),
            ("Its back", 2),
        ],
        _ => &[("Card", 0)],
    };
    images
        .iter()
        .zip(card.readings())
        .map(|(&(label, part), reading)| Image {
            label: label.into(),
            part,
            reading,
        })
        .collect()
}

/// A card file as a form, or why it cannot be one.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Opened {
    Form {
        form: Box<Form>,
        /// The parts the form can edit: a card of two has its `[back]`,
        /// `[right]` or `[spell]` as side 1.
        parts: Parts,
    },
    Refused {
        message: String,
    },
}

/// Portrait, as Phyrexian Metamorph's paper.
#[derive(Debug, Clone, Copy, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum SheetPaper {
    A4,
    A3,
    Letter,
    Legal,
}

/// The same parameters as a proxy sheet, so the two print alike.
#[derive(Debug, Clone, Copy, Facet)]
#[facet(rename_all = "camelCase")]
pub struct CardSheet {
    pub paper: SheetPaper,
    pub gap_mm: f32,
    pub margin_mm: f32,
    pub cut_marks: bool,
}

/// One card on a sheet: its text, and its art's bytes when it names some.
#[derive(Debug, Facet)]
pub struct SheetCard {
    pub text: String,
    pub qty: u32,
}

fn parse(text: &str) -> anyhow::Result<Card> {
    // The art stays as the file names it; the page fetched it by that path.
    Card::parse(text, Path::new(""))
}

fn js(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{e:#}"))
}

pub fn check_text(text: &str) -> Checked {
    match parse(text) {
        Err(e) => Checked::Refused {
            message: format!("{e:#}"),
        },
        Ok(card) => {
            let reading = read(&card);
            Checked::Card {
                arts: card
                    .faces()
                    .iter()
                    .filter_map(|f| f.art.as_ref())
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
                images: images_of(&card),
                lines: reading.lines as u32,
                unread: reading
                    .unread
                    .into_iter()
                    .map(|u| Unread {
                        line: u.line,
                        read: u.read,
                        at: u.at,
                    })
                    .collect(),
                name: card.name(),
                type_line: card.faces()[0].type_line.clone(),
            }
        }
    }
}

/// JSON of [`Checked`]: the card `text` says, or why it says none, and what
/// Insight made of its rules text.
#[wasm_bindgen]
pub fn check(text: &str) -> String {
    facet_json::to_string(&check_text(text)).expect("Checked serialises")
}

fn side(n: u32) -> Side {
    match n {
        0 => Side::Front,
        1 => Side::Second,
        _ => Side::Third,
    }
}

/// `cost`'s symbols in the order Oracle text writes them, for the card form's
/// cost buttons to add a symbol where a printed cost would have it.
#[wasm_bindgen]
pub fn sort_cost(cost: &str) -> String {
    mockery_core::cost::in_order(cost)
}

/// One face of the file as the form's fields, JSON of [`Opened`]: the custom
/// cards page edits a card either as its file or as a form, and opens the
/// form from this. `face` 0 is the top level, 1 the `[back]`, `[right]`,
/// `[spell]`, `[flipped]` or `[counterpart]`, and 2 a meld pair's `[melded]`.
#[wasm_bindgen]
pub fn form_of(text: &str, face: u32) -> String {
    let opened = match Form::of_side(text, side(face)) {
        Ok(form) => Opened::Form {
            form: Box::new(form),
            parts: Parts::of(text).unwrap_or(Parts::One),
        },
        Err(e) => Opened::Refused {
            message: format!("{e:#}"),
        },
    };
    facet_json::to_string(&opened).expect("Opened serialises")
}

/// `text` with the form's fields (JSON of `Form`) written into face `face`,
/// the rest of the file, the other face and the layout as they were.
#[wasm_bindgen]
pub fn write_form(text: &str, form: &str, face: u32) -> Result<String, JsError> {
    let form: Form =
        facet_json::from_str(form).map_err(|e| JsError::new(&format!("not a Form: {e}")))?;
    form.write_side(text, side(face)).map_err(js)
}

/// Art bytes by the path a card names them by.
fn arts(paths: &str, images: &[u8], ends: &[u32]) -> Result<HashMap<PathBuf, Vec<u8>>, JsError> {
    let paths: Vec<String> =
        facet_json::from_str(paths).map_err(|e| JsError::new(&format!("not string[]: {e}")))?;
    if paths.len() != ends.len() {
        return Err(JsError::new("one end per art path"));
    }
    let mut out = HashMap::new();
    let mut start = 0;
    for (path, &end) in paths.into_iter().zip(ends) {
        let bytes = images
            .get(start..end as usize)
            .ok_or_else(|| JsError::new("art ends past the bytes given"))?;
        out.insert(PathBuf::from(path), bytes.to_vec());
        start = end as usize;
    }
    Ok(out)
}

/// The card's face `face` as a PNG at `ppi`: 0 is the front, 1 a
/// double-faced card's back. The art is packed as for [`sheet_pdf`]; a face
/// whose art is not among it is drawn without, and the page says so.
#[wasm_bindgen]
pub fn render_png(
    text: &str,
    art_paths: &str,
    images: &[u8],
    ends: &[u32],
    face: u32,
    ppi: f32,
) -> Result<Vec<u8>, JsError> {
    let card = parse(text).map_err(js)?;
    let arts = arts(art_paths, images, ends)?;
    let doc = render::compile_with_art(std::slice::from_ref(&card), Layout::Cards, |path| {
        Ok(arts.get(path).cloned())
    })
    .map_err(js)?;
    render::png(&doc, face as usize, ppi).map_err(js)
}

/// A sheet to print and cut, as a PDF. `cards` is JSON of `SheetCard[]`,
/// `layout` of [`CardSheet`], and the art is every distinct art path (JSON of
/// `string[]`) with its bytes end to end in `images`, the `n`th ending at
/// `ends[n]`.
#[wasm_bindgen]
pub fn sheet_pdf(
    cards: &str,
    layout: &str,
    art_paths: &str,
    images: &[u8],
    ends: &[u32],
) -> Result<Vec<u8>, JsError> {
    let wanted: Vec<SheetCard> =
        facet_json::from_str(cards).map_err(|e| JsError::new(&format!("not SheetCard[]: {e}")))?;
    let l: CardSheet =
        facet_json::from_str(layout).map_err(|e| JsError::new(&format!("not a CardSheet: {e}")))?;
    let sheet = Sheet::new(
        match l.paper {
            SheetPaper::A4 => Paper::A4,
            SheetPaper::A3 => Paper::A3,
            SheetPaper::Letter => Paper::Letter,
            SheetPaper::Legal => Paper::Legal,
        },
        l.gap_mm,
        l.margin_mm,
        l.cut_marks,
    )
    .map_err(js)?;
    let mut all = Vec::new();
    for w in &wanted {
        let card = parse(&w.text).map_err(js)?;
        all.extend(std::iter::repeat_n(card, w.qty as usize));
    }
    if all.is_empty() {
        return Err(JsError::new("no cards to print"));
    }
    let arts = arts(art_paths, images, ends)?;
    // A double-faced card takes two slots, its back beside its front.
    let doc = render::compile_with_art(&all, Layout::Sheet(sheet), |path| {
        arts.get(path)
            .cloned()
            .map(Some)
            .ok_or_else(|| anyhow!("no art {} in the repo", path.display()))
    })
    .map_err(js)?;
    render::pdf(&doc).map_err(js)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../meldweb-curator/web/src/mockery.gen.ts"
    );
    const DRONE: &str = include_str!("../../core/tests/cards/proofsheet-drone.card.toml");

    #[test]
    fn generated_typescript_is_current() {
        let mut g = facet_typescript::TypeScriptGenerator::new();
        g.add_type::<Checked>();
        g.add_type::<CardSheet>();
        g.add_type::<SheetCard>();
        g.add_type::<Opened>();
        let want = format!(
            "// Generated from crates/mirage-mockery/wasm/src/lib.rs. Do not edit:\n\
             // UPDATE_TS=1 cargo test -p mockery-wasm rewrites it.\n\n{}",
            g.finish().trim_end()
        ) + "\n";
        if std::env::var_os("UPDATE_TS").is_some() {
            std::fs::write(GENERATED, &want).unwrap();
        }
        let have = std::fs::read_to_string(GENERATED).unwrap_or_default();
        assert!(
            have == want,
            "{GENERATED} is stale; run UPDATE_TS=1 cargo test -p mockery-wasm"
        );
    }

    #[test]
    fn a_card_reads_with_its_name() {
        match check_text(DRONE) {
            Checked::Card { name, .. } => assert_eq!(name, "Proofsheet Drone"),
            Checked::Refused { message } => panic!("{message}"),
        }
    }

    #[test]
    fn a_form_opens_from_the_file_and_writes_back_into_it() {
        let opened = form_of(DRONE, 0);
        let form = opened
            .strip_prefix(r#"{"kind":"form","form":"#)
            .and_then(|f| f.strip_suffix(r#","parts":"one"}"#))
            .unwrap_or_else(|| panic!("{opened}"));
        assert_eq!(
            write_form(DRONE, form, 0).unwrap_or_else(|_| panic!()),
            DRONE
        );
    }

    #[test]
    fn a_card_renders_to_a_png() {
        let png = render_png(DRONE, "[]", &[], &[], 0, 72.0).unwrap_or_else(|_| panic!("render"));
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn a_double_faced_card_renders_each_face_and_says_it_has_two() {
        const SHINOVAR: &str =
            include_str!("../../core/tests/cards/the-cleansing-of-shinovar.card.toml");
        match check_text(SHINOVAR) {
            Checked::Card { name, images, .. } => {
                assert_eq!(
                    name,
                    "The Cleansing of Shinovar // Kaladin, Herald of Second Chances"
                );
                assert_eq!(images.len(), 2);
            }
            Checked::Refused { message } => panic!("{message}"),
        }
        let front = render_png(SHINOVAR, "[]", &[], &[], 0, 72.0).unwrap_or_else(|_| panic!());
        let back = render_png(SHINOVAR, "[]", &[], &[], 1, 72.0).unwrap_or_else(|_| panic!());
        assert_ne!(front, back);
    }

    #[test]
    fn a_card_with_a_spell_is_one_image_and_two_forms() {
        const SKYBREAKER: &str =
            include_str!("../../core/tests/cards/skybreaker-initiate.card.toml");
        match check_text(SKYBREAKER) {
            Checked::Card { name, images, .. } => {
                assert_eq!(name, "Skybreaker Initiate // Full Lashing");
                assert_eq!(images.len(), 1);
            }
            Checked::Refused { message } => panic!("{message}"),
        }
        let spell = form_of(SKYBREAKER, 1);
        assert!(spell.contains(r#""name":"Full Lashing""#), "{spell}");
        assert!(spell.ends_with(r#","parts":"adventure"}"#), "{spell}");
    }

    #[test]
    fn a_meld_pair_is_four_images_and_three_forms() {
        const SMUGGLER: &str = include_str!("../../core/tests/cards/corellian-smuggler.card.toml");
        match check_text(SMUGGLER) {
            Checked::Card { images, .. } => {
                let parts: Vec<u32> = images.iter().map(|i| i.part).collect();
                assert_eq!(parts, [0, 2, 1, 2]);
            }
            Checked::Refused { message } => panic!("{message}"),
        }
        let melded = form_of(SMUGGLER, 2);
        assert!(melded.contains(r#""name":"The Kessel Runner""#), "{melded}");
        assert!(melded.ends_with(r#","parts":"meld"}"#), "{melded}");
        let back = render_png(SMUGGLER, "[]", &[], &[], 3, 72.0).unwrap_or_else(|_| panic!());
        assert_eq!(&back[1..4], b"PNG");
    }

    #[test]
    fn each_image_says_which_way_it_is_read() {
        use mockery_core::card::Turn::{Anticlockwise, Clockwise};
        use Reading::*;
        let readings = |text: &str| match check_text(text) {
            Checked::Card { images, .. } => {
                let card = parse(text).unwrap();
                let doc = render::compile(std::slice::from_ref(&card), Layout::Cards).unwrap();
                assert_eq!(doc.pages().len(), images.len());
                images.into_iter().map(|i| i.reading).collect::<Vec<_>>()
            }
            Checked::Refused { message } => panic!("{message}"),
        };
        let card = |name| {
            std::fs::read_to_string(format!(
                "{}/../core/tests/cards/{name}.card.toml",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap()
        };
        let clockwise = Turned { turn: Clockwise };
        let anticlockwise = Turned {
            turn: Anticlockwise,
        };
        assert_eq!(readings(DRONE), [Upright]);
        assert_eq!(readings(&card("herdazian-squire")), [Upright]);
        assert_eq!(readings(&card("force-push-force-pull")), [clockwise]);
        assert_eq!(
            readings(&card("detention-block-trash-compactor")),
            [clockwise]
        );
        assert_eq!(
            readings(&card("chasm-run-gemhearts")),
            [LowerTurned {
                turn: Anticlockwise
            }]
        );
        assert_eq!(readings(&card("siege-of-narak")), [clockwise, Upright]);
        assert_eq!(
            readings(&card("corellian-smuggler")),
            [Upright, anticlockwise, Upright, anticlockwise]
        );
    }
}
