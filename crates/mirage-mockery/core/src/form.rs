//! A `.card.toml` as the fields of a form, and a form's fields written back
//! into the file they came from. Curator's custom cards page edits a card
//! either way, the file or the form, and switches between them freely.
//!
//! Writing leaves the rest of the file as it was: comments, order and the
//! fields the form did not change survive, so an edit is the diff it looks
//! like. A form is read from what the file says, not from what [`Card::parse`]
//! makes of it: a creature without its power and toughness yet is a form
//! still being filled in, and the page says what is wrong with it.
//!
//! [`Card::parse`]: crate::card::Card::parse

use anyhow::{anyhow, bail, Result};
use facet::Facet;
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::card::{RawCard, RawFace};

/// Which face the form edits: the top level, or a two-faced card's other face,
/// its `[back]`, its `[right]` half, the `[spell]` inset in it or its
/// `[flipped]` half, or a meld pair's `[counterpart]` (second) and
/// `[melded]` card (third). The layout and the faces the form is not
/// editing are left as the file has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Front,
    Second,
    Third,
}

/// What a card file's parts are, by the table its second part is in: what
/// the form's face picker names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(rename_all = "camelCase")]
pub enum Parts {
    /// One face.
    One,
    /// A front and a `[back]`.
    FrontBack,
    /// A left and a `[right]` half.
    Halves,
    /// A card and the Adventure in its `[spell]`.
    Adventure,
    /// A card and the Omen in its `[spell]`.
    Omen,
    /// A card and the prepare spell in its `[spell]`.
    Prepare,
    /// An upright half and a `[flipped]` one.
    Flip,
    /// A meld pair: a card, its `[counterpart]` and the `[melded]` card.
    Meld,
}

impl Parts {
    /// The file's parts, or why it is no card file.
    pub fn of(text: &str) -> Result<Self> {
        let raw = RawCard::parse(text)?;
        Ok(match second_key(&raw) {
            Err(_) if raw.melded.is_some() => Parts::Meld,
            Err(_) => Parts::One,
            Ok("back") => Parts::FrontBack,
            Ok("right") => Parts::Halves,
            Ok("flipped") => Parts::Flip,
            Ok("counterpart") => Parts::Meld,
            // A spell whose layout is not settled yet is named as the
            // commoner of the three; an omen card's layout is an
            // adventurer's, and its spell's Omen subtype says which.
            Ok(_) if raw.layout.as_deref().map(str::trim) == Some("prepare") => Parts::Prepare,
            Ok(_)
                if raw.spell.as_ref().is_some_and(|s| {
                    s.type_line
                        .split_whitespace()
                        .skip_while(|w| *w != "—")
                        .any(|w| w == "Omen")
                }) =>
            {
                Parts::Omen
            }
            Ok(_) => Parts::Adventure,
        })
    }
}

/// The table a card keeps its second face in, by what the file has.
fn second_key(raw: &RawCard) -> Result<&'static str> {
    [
        ("back", &raw.back),
        ("right", &raw.right),
        ("spell", &raw.spell),
        ("flipped", &raw.flipped),
        ("counterpart", &raw.counterpart),
    ]
    .into_iter()
    .find_map(|(key, face)| face.is_some().then_some(key))
    .ok_or_else(|| anyhow!("this card has one face"))
}

/// The table `side` is in, the top level being none.
fn key_of(raw: &RawCard, side: Side) -> Result<Option<&'static str>> {
    match side {
        Side::Front => Ok(None),
        Side::Second => second_key(raw).map(Some),
        Side::Third if raw.melded.is_some() => Ok(Some("melded")),
        Side::Third => bail!("only a meld pair has a third part, its [melded] card"),
    }
}

/// Every field of a card file. An empty string is a field the file leaves out.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[facet(rename_all = "camelCase")]
pub struct Form {
    pub name: String,
    pub cost: String,
    pub type_line: String,
    pub rules: String,
    pub flavor: String,
    pub corner: FormCorner,
    /// The colour indicator's letters in WUBRG, like `"WR"`.
    pub color_indicator: String,
    pub art: String,
    pub artist: String,
}

/// The bottom-right corner, one of them at most, as [`crate::card::Corner`].
/// Power and toughness are apart here, as the form asks for them.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum FormCorner {
    None,
    Pt { power: String, toughness: String },
    Loyalty { value: String },
    Defense { value: String },
}

impl Form {
    /// The file's fields, or why the form cannot hold them: it is not TOML,
    /// or it says something no form field says.
    pub fn of(text: &str) -> Result<Self> {
        Self::of_side(text, Side::Front)
    }

    /// [`Form::of`] for either face.
    pub fn of_side(text: &str, side: Side) -> Result<Self> {
        // facet_toml reads `type = ` with nothing after it as empty, and the
        // form must be able to write back what it opened.
        text.parse::<DocumentMut>()?;
        let card = RawCard::parse(text)?;
        let raw = match key_of(&card, side)? {
            None => card.front,
            Some(key) => match key {
                "back" => card.back,
                "right" => card.right,
                "spell" => card.spell,
                "flipped" => card.flipped,
                "counterpart" => card.counterpart,
                _ => card.melded,
            }
            .expect("the key is a table the file has"),
        };
        Self::of_face(raw)
    }

    fn of_face(raw: RawFace) -> Result<Self> {
        let corner = match (raw.pt, raw.loyalty, raw.defense) {
            (None, None, None) => FormCorner::None,
            (Some(pt), None, None) => {
                let (power, toughness) = pt.split_once('/').unwrap_or((&pt, ""));
                FormCorner::Pt {
                    power: power.trim().into(),
                    toughness: toughness.trim().into(),
                }
            }
            (None, Some(value), None) => FormCorner::Loyalty { value },
            (None, None, Some(value)) => FormCorner::Defense { value },
            _ => bail!("a card has at most one of pt, loyalty and defense"),
        };
        Ok(Form {
            name: raw.name,
            cost: raw.cost.unwrap_or_default(),
            type_line: raw.type_line,
            rules: raw.rules.unwrap_or_default(),
            flavor: raw.flavor.unwrap_or_default(),
            corner,
            color_indicator: raw.color_indicator.unwrap_or_default(),
            art: raw
                .art
                .map(|a| a.to_string_lossy().into_owned())
                .unwrap_or_default(),
            artist: raw.artist.unwrap_or_default(),
        })
    }

    /// `text` with this form's fields in it. Fields already as the form has
    /// them are not touched, so their quoting and their comments stay.
    pub fn write_into(&self, text: &str) -> Result<String> {
        self.write_side(text, Side::Front)
    }

    /// [`Form::write_into`] for either face. Writing one face never touches
    /// the other, nor the layout.
    pub fn write_side(&self, text: &str, side: Side) -> Result<String> {
        let mut doc: DocumentMut = text.parse()?;
        let table = match key_of(&RawCard::parse(text)?, side)? {
            None => doc.as_table_mut(),
            Some(key) => {
                let Some(table) = doc.get_mut(key).and_then(Item::as_table_mut) else {
                    bail!("the form edits a [{key}] table, not an inline one");
                };
                table
            }
        };
        let (pt, loyalty, defense) = match &self.corner {
            FormCorner::None => (None, None, None),
            FormCorner::Pt { power, toughness } => {
                (Some(format!("{power}/{toughness}")), None, None)
            }
            FormCorner::Loyalty { value } => (None, Some(value.clone()), None),
            FormCorner::Defense { value } => (None, None, Some(value.clone())),
        };
        let present = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let fields = [
            ("name", Some(self.name.clone())),
            ("cost", present(&self.cost)),
            ("type", Some(self.type_line.clone())),
            ("rules", present(&self.rules)),
            ("flavor", present(&self.flavor)),
            ("pt", pt),
            ("loyalty", loyalty),
            ("defense", defense),
            ("color_indicator", present(&self.color_indicator)),
            ("art", present(&self.art)),
            ("artist", present(&self.artist)),
        ];
        for (key, want) in fields {
            set(table, key, want)?;
        }
        Ok(doc.to_string())
    }
}

/// How a card is laid out, as the form picks it: one face, or what
/// `layout =` declares, an adventurer and an omen card told apart by their
/// spell's subtype as [`Card::parse`] tells them. A fused, aftermath or Room
/// card is a split card its text makes so, and a battle any face its type
/// line makes one, so none is a layout of its own.
///
/// [`Card::parse`]: crate::card::Card::parse
#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(rename_all = "camelCase")]
pub enum CardLayout {
    One,
    Transform,
    Modal,
    Split,
    Adventure,
    Omen,
    Prepare,
    Flip,
    Meld,
}

impl CardLayout {
    pub const ALL: [CardLayout; 9] = [
        CardLayout::One,
        CardLayout::Transform,
        CardLayout::Modal,
        CardLayout::Split,
        CardLayout::Adventure,
        CardLayout::Omen,
        CardLayout::Prepare,
        CardLayout::Flip,
        CardLayout::Meld,
    ];

    /// What the file's `layout =` says, none for one face.
    fn word(self) -> Option<&'static str> {
        Some(match self {
            CardLayout::One => return None,
            CardLayout::Transform => "transform",
            CardLayout::Modal => "modal",
            CardLayout::Split => "split",
            // Scryfall lays an omen card out as an adventurer.
            CardLayout::Adventure | CardLayout::Omen => "adventure",
            CardLayout::Prepare => "prepare",
            CardLayout::Flip => "flip",
            CardLayout::Meld => "meld",
        })
    }

    /// The table its second part is in, none for one face. A meld pair's
    /// third is always `[melded]`.
    fn second(self) -> Option<&'static str> {
        Some(match self {
            CardLayout::One => return None,
            CardLayout::Transform | CardLayout::Modal => "back",
            CardLayout::Split => "right",
            CardLayout::Adventure | CardLayout::Omen | CardLayout::Prepare => "spell",
            CardLayout::Flip => "flipped",
            CardLayout::Meld => "counterpart",
        })
    }

    /// The layout the file declares, or, where it declares none it knows,
    /// the one its tables are for.
    pub fn of(text: &str) -> Result<Self> {
        let raw = RawCard::parse(text)?;
        Ok(match raw.layout.as_deref().map(str::trim) {
            Some("transform") => CardLayout::Transform,
            Some("modal") => CardLayout::Modal,
            Some("split") => CardLayout::Split,
            Some("prepare") => CardLayout::Prepare,
            Some("flip") => CardLayout::Flip,
            Some("meld") => CardLayout::Meld,
            Some("adventure") if spell_is_omen(&raw) => CardLayout::Omen,
            Some("adventure") => CardLayout::Adventure,
            _ => match Parts::of(text)? {
                Parts::One => CardLayout::One,
                Parts::FrontBack => CardLayout::Transform,
                Parts::Halves => CardLayout::Split,
                Parts::Adventure => CardLayout::Adventure,
                Parts::Omen => CardLayout::Omen,
                Parts::Prepare => CardLayout::Prepare,
                Parts::Flip => CardLayout::Flip,
                Parts::Meld => CardLayout::Meld,
            },
        })
    }
}

fn spell_is_omen(raw: &RawCard) -> bool {
    raw.spell
        .as_ref()
        .is_some_and(|s| subtypes(&s.type_line).contains(&"Omen"))
}

/// A type line's words after the dash.
fn subtypes(type_line: &str) -> Vec<&str> {
    type_line
        .split_whitespace()
        .skip_while(|w| *w != "—")
        .skip(1)
        .collect()
}

/// A card file laid out anew, and what of it that threw away: the parts
/// dropped and the fields the new layout has no place for, each named for
/// the page to ask before it does so.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct Relaid {
    pub text: String,
    pub lost: Vec<String>,
}

/// `text` laid out as `to`. A part it gains is filled in as a card that
/// checks, so the form opens on it; a part it keeps moves to the table the
/// layout keeps it in, and only what that layout refuses of it is
/// rewritten; a part it loses is dropped. The rest of the file, comments
/// and all, is as it was.
pub fn with_layout(text: &str, to: CardLayout) -> Result<Relaid> {
    let from = CardLayout::of(text)?;
    if from == to {
        return Ok(Relaid {
            text: text.into(),
            lost: Vec::new(),
        });
    }
    let mut doc: DocumentMut = text.parse()?;
    let mut lost = Vec::new();
    // Every second part the file has, the one the layout reads first.
    let mut seconds = ["back", "right", "spell", "flipped", "counterpart"]
        .into_iter()
        .filter_map(|key| doc.remove(key));
    let second = seconds.next();
    lost.extend(seconds.map(|item| part_name(&item)));
    let melded = doc.remove("melded");
    let root = doc.as_table_mut();
    set(root, "layout", to.word().map(String::from))?;
    if let (CardLayout::Meld, Some(counterpart), Some(melded)) = (from, &second, &melded) {
        if to != CardLayout::Meld {
            unmeld(root, counterpart, melded)?;
        }
    }
    let Some(key) = to.second() else {
        lost.extend(second.iter().chain(&melded).map(part_name));
        return Ok(Relaid {
            text: doc.to_string(),
            lost,
        });
    };
    let mut second = match second {
        Some(item) => item,
        None => seed(to, root)?,
    };
    fit(&mut second, to, &mut lost)?;
    if to == CardLayout::Meld {
        let melded = match melded {
            Some(item) => item,
            None => face(&[
                ("name", "Melded Card"),
                ("type", "Legendary Creature"),
                ("pt", "6/6"),
            ])?,
        };
        melds_them(root, &second, &melded)?;
        root.insert(key, second);
        root.insert("melded", melded);
    } else {
        lost.extend(melded.iter().map(part_name));
        root.insert(key, second);
    }
    Ok(Relaid {
        text: doc.to_string(),
        lost,
    })
}

fn part_name(item: &Item) -> String {
    item.get("name")
        .and_then(Item::as_str)
        .map_or_else(|| "a part with no name".into(), |n| format!("{n:?}"))
}

/// A table holding `fields`.
fn face(fields: &[(&str, &str)]) -> Result<Item> {
    let mut table = Table::new();
    for (key, value) in fields {
        set(&mut table, key, Some((*value).into()))?;
    }
    Ok(Item::Table(table))
}

/// A second part as `to` prints one, a card that checks beside `front`.
fn seed(to: CardLayout, front: &Table) -> Result<Item> {
    let front_type = front.get("type").and_then(Item::as_str).unwrap_or("");
    let words: Vec<&str> = front_type.split_whitespace().collect();
    face(&match to {
        CardLayout::One => bail!("one face has no second part"),
        CardLayout::Transform | CardLayout::Modal => {
            vec![("name", "Back Face"), ("type", "Creature"), ("pt", "1/1")]
        }
        // A Room's doors share one type line, and a spell's other half is
        // most often a spell of its kind.
        CardLayout::Split => {
            let rooms = words.contains(&"Room");
            let spell = matches!(words.first(), Some(&"Instant" | &"Sorcery"));
            let type_line = if rooms || spell {
                front_type
            } else {
                "Instant"
            };
            vec![("name", "Right Half"), ("cost", "{1}"), ("type", type_line)]
        }
        CardLayout::Adventure => vec![
            ("name", "Adventure Spell"),
            ("cost", "{1}"),
            ("type", "Sorcery — Adventure"),
        ],
        CardLayout::Omen => vec![
            ("name", "Omen Spell"),
            ("cost", "{1}"),
            ("type", "Sorcery — Omen"),
        ],
        CardLayout::Prepare => vec![
            ("name", "Prepare Spell"),
            ("cost", "{1}"),
            ("type", "Sorcery"),
        ],
        CardLayout::Flip => vec![
            ("name", "Flipped Half"),
            ("type", "Legendary Creature"),
            ("pt", "1/1"),
        ],
        CardLayout::Meld => vec![("name", "Counterpart"), ("type", "Creature"), ("pt", "1/1")],
    })
}

/// The second part rewritten where `to` refuses it as it is, each field
/// that goes named in `lost`.
fn fit(item: &mut Item, to: CardLayout, lost: &mut Vec<String>) -> Result<()> {
    let name = part_name(item);
    let Some(t) = item.as_table_like_mut() else {
        bail!("the second part is a table, not {}", item.type_name());
    };
    let type_line = t
        .get("type")
        .and_then(Item::as_str)
        .unwrap_or("")
        .to_string();
    let (types, subs) = match type_line.split_once('—') {
        Some((types, subs)) => (types.trim(), subs.trim()),
        None => (type_line.trim(), ""),
    };
    let spell = matches!(types, "Instant" | "Sorcery");
    // An Adventure or an Omen subtype is an inset spell's, and only its.
    let kept: Vec<&str> = subs
        .split_whitespace()
        .filter(|w| !matches!(*w, "Adventure" | "Omen"))
        .collect();
    let mut drop = |t: &mut dyn TableLike, fields: &[&str]| {
        for field in fields {
            if t.remove(field).is_some() {
                lost.push(format!("{name}'s {field}"));
            }
        }
    };
    let joined = |types: &str, subs: &[&str]| match subs {
        [] => types.to_string(),
        _ => format!("{types} — {}", subs.join(" ")),
    };
    let new_type = match to {
        CardLayout::Adventure | CardLayout::Omen | CardLayout::Prepare => {
            // Drawn in the card's text box, an instant or sorcery with a
            // cost and no corner or art of its own.
            drop(
                t,
                &[
                    "pt",
                    "loyalty",
                    "defense",
                    "art",
                    "artist",
                    "color_indicator",
                ],
            );
            if t.get("cost").is_none() {
                set(t, "cost", Some("{1}".into()))?;
            }
            let mut subs = if spell { kept } else { Vec::new() };
            subs.extend(match to {
                CardLayout::Adventure => Some("Adventure"),
                CardLayout::Omen => Some("Omen"),
                _ => None,
            });
            joined(
                if types == "Instant" {
                    "Instant"
                } else {
                    "Sorcery"
                },
                &subs,
            )
        }
        // A permanent's half, sharing the upright half's cost, colour and art.
        CardLayout::Flip => {
            drop(
                t,
                &[
                    "cost",
                    "color_indicator",
                    "art",
                    "artist",
                    "loyalty",
                    "defense",
                ],
            );
            if spell {
                "Creature".into()
            } else {
                joined(types, &kept)
            }
        }
        // Each card of a meld pair is a permanent.
        CardLayout::Meld if spell => "Creature".into(),
        CardLayout::Split => {
            if t.get("cost").is_none() {
                set(t, "cost", Some("{1}".into()))?;
            }
            joined(types, &kept)
        }
        _ => joined(types, &kept),
    };
    if new_type != type_line {
        // Its subtypes follow its card types; only a change of card type
        // is something the page asks about.
        if new_type.split('—').next().map(str::trim) != Some(types) {
            lost.push(format!("{name}'s type {type_line:?}"));
        }
        set(t, "type", Some(new_type.clone()))?;
    }
    let creature = new_type.split_whitespace().any(|w| w == "Creature");
    if creature && t.get("pt").is_none() {
        set(t, "pt", Some("1/1".into()))?;
    }
    Ok(())
}

/// One card of a meld pair melds them, naming the card they meld into
/// (CR 712.4a), and the top level is that card: it is given the line if it
/// has none.
fn melds_them(front: &mut Table, counterpart: &Item, melded: &Item) -> Result<()> {
    let line = meld_line(front, counterpart, melded);
    let named = name_of(melded);
    let rules = str_of(front.get("rules"));
    if rules
        .lines()
        .any(|l| l.to_lowercase().contains("meld") && l.contains(named.as_str()))
    {
        return Ok(());
    }
    let mut with = rules.trim_end().to_string();
    if !with.is_empty() {
        with.push('\n');
    }
    with += &line;
    if rules.ends_with('\n') {
        with.push('\n');
    }
    set(front, "rules", Some(with))
}

/// The line [`melds_them`] gives a card taken back out of it, as the pair
/// goes; a line of the user's own is theirs to keep or not.
fn unmeld(front: &mut Table, counterpart: &Item, melded: &Item) -> Result<()> {
    let line = meld_line(front, counterpart, melded);
    let rules = str_of(front.get("rules"));
    let without = if rules.contains(&format!("\n{line}")) {
        rules.replacen(&format!("\n{line}"), "", 1)
    } else if rules.starts_with(&line) {
        rules.replacen(&line, "", 1)
    } else {
        return Ok(());
    };
    set(
        front,
        "rules",
        (!without.trim().is_empty()).then_some(without),
    )
}

fn meld_line(front: &Table, counterpart: &Item, melded: &Item) -> String {
    let this = front.get("name").and_then(Item::as_str).unwrap_or("this");
    format!(
        "At the beginning of your end step, if you both own and control {this} and a \
         permanent named {}, exile them, then meld them into {}.",
        name_of(counterpart),
        name_of(melded)
    )
}

fn name_of(item: &Item) -> String {
    str_of(item.get("name"))
}

fn str_of(item: Option<&Item>) -> String {
    item.and_then(Item::as_str).unwrap_or("").to_string()
}

fn set(doc: &mut dyn TableLike, key: &str, want: Option<String>) -> Result<()> {
    let Some(want) = want else {
        doc.remove(key);
        return Ok(());
    };
    if doc.get(key).and_then(Item::as_str) == Some(want.as_str()) {
        return Ok(());
    }
    // Rules text is a paragraph a line, so it is written as lines.
    let value: Value = if key == "rules" || want.contains('\n') {
        quote_multiline(&want).parse()?
    } else {
        want.into()
    };
    match doc.get_mut(key) {
        Some(Item::Value(v)) => {
            // Keep the comment hung after the old value.
            let decor = v.decor().clone();
            *v = value;
            *v.decor_mut() = decor;
        }
        _ => {
            doc.insert(key, Item::Value(value));
        }
    }
    Ok(())
}

/// A TOML multi-line basic string, the same text read back: the newline after
/// the opening quotes is not part of it, so one goes there.
fn quote_multiline(s: &str) -> String {
    let mut out = String::from("\"\"\"\n");
    for ch in s.chars() {
        match ch {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' | '\t' => out.push(ch),
            c if c.is_control() => out += &format!("\\u{:04X}", c as u32),
            c => out.push(c),
        }
    }
    out += "\"\"\"";
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const DRONE: &str = include_str!("../tests/cards/proofsheet-drone.card.toml");

    #[test]
    fn a_card_reads_as_its_fields() {
        let form = Form::of(DRONE).unwrap();
        assert_eq!(form.name, "Proofsheet Drone");
        assert_eq!(form.cost, "{2}{U}{U/P}");
        assert_eq!(
            form.corner,
            FormCorner::Pt {
                power: "2".into(),
                toughness: "3".into()
            }
        );
        assert!(form.rules.starts_with("Flying\n"));
        assert_eq!(form.art, "");
    }

    #[test]
    fn an_unchanged_form_writes_the_file_back_as_it_was() {
        let text = format!("# my drone\n{DRONE}");
        let form = Form::of(&text).unwrap();
        assert_eq!(form.write_into(&text).unwrap(), text);
    }

    #[test]
    fn a_change_touches_only_its_field_and_keeps_its_comment() {
        let text = "name = \"A\" # the name\ntype = \"Instant\"\ncost = \"{U}\" # cheap\n";
        let mut form = Form::of(text).unwrap();
        form.cost = "{1}{U}".into();
        assert_eq!(
            form.write_into(text).unwrap(),
            "name = \"A\" # the name\ntype = \"Instant\"\ncost = \"{1}{U}\" # cheap\n"
        );
    }

    #[test]
    fn a_new_corner_replaces_the_old() {
        let mut form = Form::of(DRONE).unwrap();
        form.type_line = "Legendary Planeswalker — Drone".into();
        form.corner = FormCorner::Loyalty { value: "4".into() };
        let written = form.write_into(DRONE).unwrap();
        assert!(!written.contains("pt ="));
        assert_eq!(Form::of(&written).unwrap(), form);
    }

    #[test]
    fn an_emptied_field_leaves_the_file() {
        let mut form = Form::of(DRONE).unwrap();
        form.flavor.clear();
        form.artist.clear();
        let written = form.write_into(DRONE).unwrap();
        assert!(!written.contains("flavor") && !written.contains("artist"));
    }

    #[test]
    fn a_colour_indicator_is_a_field_and_survives_other_edits() {
        let text = "name = \"Kaladin\"\ntype = \"Creature — Human\"\npt = \"4/4\"\ncolor_indicator = \"WR\" # the back face's\n";
        let mut form = Form::of(text).unwrap();
        assert_eq!(form.color_indicator, "WR");
        form.name = "Kaladin, Herald".into();
        let written = form.write_into(text).unwrap();
        assert!(
            written.contains("color_indicator = \"WR\" # the back face's"),
            "{written}"
        );
        form.color_indicator.clear();
        assert!(!form.write_into(text).unwrap().contains("color_indicator"));
    }

    #[test]
    fn a_file_the_form_cannot_hold_is_refused() {
        assert!(
            Form::of("name = \"A\"\ntype = \"Creature\"\npt = \"1/1\"\nloyalty = \"3\"\n").is_err()
        );
        assert!(Form::of("name = \"A\"\ntype = \"Instant\"\nmana = \"{U}\"\n").is_err());
        assert!(Form::of("name = \"A\"\ntype = ").is_err());
    }

    const SHINOVAR: &str = include_str!("../tests/cards/the-cleansing-of-shinovar.card.toml");
    const FORCE: &str = include_str!("../tests/cards/force-push-force-pull.card.toml");
    const SKYBREAKER: &str = include_str!("../tests/cards/skybreaker-initiate.card.toml");
    const FABRIALIST: &str = include_str!("../tests/cards/ardent-fabrialist.card.toml");
    const SANTHID: &str = include_str!("../tests/cards/purelake-santhid.card.toml");
    const CHASM: &str = include_str!("../tests/cards/chasm-run-gemhearts.card.toml");
    const DETENTION: &str =
        include_str!("../tests/cards/detention-block-trash-compactor.card.toml");
    const SQUIRE: &str = include_str!("../tests/cards/herdazian-squire.card.toml");
    const SMUGGLER: &str = include_str!("../tests/cards/corellian-smuggler.card.toml");

    #[test]
    fn editing_the_front_leaves_the_layout_and_the_second_face_alone() {
        for text in [SHINOVAR, FORCE, SKYBREAKER, FABRIALIST, SQUIRE, SMUGGLER] {
            let mut form = Form::of(text).unwrap();
            form.name = "Renamed".into();
            form.flavor = "New flavour.".into();
            let written = form.write_into(text).unwrap();
            let (_, tail) = text.split_once("\n[").unwrap();
            let (_, written_tail) = written.split_once("\n[").unwrap();
            assert_eq!(written_tail, tail, "{written}");
            assert!(written.contains("layout = "));
            assert_eq!(Form::of(&written).unwrap(), form);
            assert_eq!(
                Form::of_side(&written, Side::Second).unwrap(),
                Form::of_side(text, Side::Second).unwrap()
            );
        }
    }

    #[test]
    fn the_second_face_is_a_form_too() {
        let mut back = Form::of_side(SHINOVAR, Side::Second).unwrap();
        assert_eq!(back.name, "Kaladin, Herald of Second Chances");
        assert_eq!(back.color_indicator, "WR");
        back.corner = FormCorner::Pt {
            power: "5".into(),
            toughness: "5".into(),
        };
        let written = back.write_side(SHINOVAR, Side::Second).unwrap();
        assert!(written.contains("pt = \"5/5\""));
        assert_eq!(Form::of(&written).unwrap(), Form::of(SHINOVAR).unwrap());
        assert_eq!(Form::of_side(&written, Side::Second).unwrap(), back);
        let unchanged = Form::of_side(FORCE, Side::Second).unwrap();
        assert_eq!(unchanged.write_side(FORCE, Side::Second).unwrap(), FORCE);
        assert!(Form::of_side(DRONE, Side::Second).is_err());
    }

    #[test]
    fn a_file_names_its_parts() {
        for (text, parts) in [
            (DRONE, Parts::One),
            (SHINOVAR, Parts::FrontBack),
            (FORCE, Parts::Halves),
            (SKYBREAKER, Parts::Adventure),
            (FABRIALIST, Parts::Prepare),
            (SANTHID, Parts::Omen),
            (CHASM, Parts::Halves),
            (DETENTION, Parts::Halves),
            (SQUIRE, Parts::Flip),
            (SMUGGLER, Parts::Meld),
        ] {
            assert_eq!(Parts::of(text).unwrap(), parts);
        }
    }

    #[test]
    fn an_inset_spell_is_the_second_face() {
        for text in [SKYBREAKER, FABRIALIST, SANTHID] {
            let mut spell = Form::of_side(text, Side::Second).unwrap();
            assert!(["Instant — Adventure", "Sorcery", "Instant — Omen"]
                .contains(&spell.type_line.as_str()));
            assert_eq!(spell.write_side(text, Side::Second).unwrap(), text);
            spell.cost = "{2}{U}".into();
            let written = spell.write_side(text, Side::Second).unwrap();
            assert_eq!(Form::of_side(&written, Side::Second).unwrap(), spell);
            assert_eq!(Form::of(&written).unwrap(), Form::of(text).unwrap());
        }
    }

    #[test]
    fn a_meld_pair_is_three_forms_and_each_writes_only_itself() {
        let parts = [Side::Front, Side::Second, Side::Third];
        let names: Vec<String> = parts
            .iter()
            .map(|&side| Form::of_side(SMUGGLER, side).unwrap().name)
            .collect();
        assert_eq!(
            names,
            [
                "Corellian Smuggler",
                "Battered Freighter",
                "The Kessel Runner"
            ]
        );
        let mut melded = Form::of_side(SMUGGLER, Side::Third).unwrap();
        melded.corner = FormCorner::Pt {
            power: "9".into(),
            toughness: "9".into(),
        };
        let written = melded.write_side(SMUGGLER, Side::Third).unwrap();
        assert_eq!(Form::of_side(&written, Side::Third).unwrap(), melded);
        for side in [Side::Front, Side::Second] {
            assert_eq!(
                Form::of_side(&written, side).unwrap(),
                Form::of_side(SMUGGLER, side).unwrap()
            );
        }
        // Only a meld pair has a third part.
        assert!(Form::of_side(SQUIRE, Side::Third).is_err());
        assert_eq!(
            Form::of_side(SQUIRE, Side::Second).unwrap().name,
            "The Lopen, Arm Regrown"
        );
    }

    const CHASMFIEND: &str = include_str!("../tests/cards/chunky-chasmfiend.card.toml");

    fn checks(text: &str) -> Result<crate::card::Card> {
        crate::card::Card::parse(text, std::path::Path::new(""))
    }

    #[test]
    fn a_file_names_its_layout() {
        for (text, layout) in [
            (DRONE, CardLayout::One),
            (SHINOVAR, CardLayout::Transform),
            (CHASMFIEND, CardLayout::Modal),
            (FORCE, CardLayout::Split),
            (DETENTION, CardLayout::Split),
            (SKYBREAKER, CardLayout::Adventure),
            (SANTHID, CardLayout::Omen),
            (FABRIALIST, CardLayout::Prepare),
            (SQUIRE, CardLayout::Flip),
            (SMUGGLER, CardLayout::Meld),
        ] {
            assert_eq!(CardLayout::of(text).unwrap(), layout, "{text}");
        }
    }

    #[test]
    fn a_layout_a_card_has_already_changes_nothing() {
        for text in [DRONE, SHINOVAR, SKYBREAKER, SMUGGLER] {
            let relaid = with_layout(text, CardLayout::of(text).unwrap()).unwrap();
            assert_eq!(relaid.text, text);
            assert!(relaid.lost.is_empty());
        }
    }

    #[test]
    fn one_face_laid_out_anew_gains_a_part_that_checks_and_the_form_opens() {
        let text = format!(
            "# my drone\n{}",
            DRONE.replace("pt = ", "pt = \"2/3\" # small\n#pt = ")
        );
        for to in CardLayout::ALL {
            let relaid = with_layout(&text, to).unwrap();
            assert!(relaid.lost.is_empty(), "{to:?}: {:?}", relaid.lost);
            assert_eq!(CardLayout::of(&relaid.text).unwrap(), to, "{}", relaid.text);
            if let Err(e) = checks(&relaid.text) {
                panic!("{to:?}: {e:#}\n{}", relaid.text);
            }
            assert!(relaid.text.starts_with("# my drone\n"), "{}", relaid.text);
            assert!(
                relaid.text.contains("pt = \"2/3\" # small\n"),
                "{}",
                relaid.text
            );
            if to != CardLayout::Meld {
                assert_eq!(Form::of(&relaid.text).unwrap(), Form::of(&text).unwrap());
            }
            if to != CardLayout::One {
                Form::of_side(&relaid.text, Side::Second).unwrap();
            }
        }
        let seeded =
            |to| Form::of_side(&with_layout(&text, to).unwrap().text, Side::Second).unwrap();
        assert_eq!(
            seeded(CardLayout::Adventure).type_line,
            "Sorcery — Adventure"
        );
        assert_eq!(seeded(CardLayout::Adventure).cost, "{1}");
        assert_eq!(seeded(CardLayout::Omen).type_line, "Sorcery — Omen");
        assert_eq!(seeded(CardLayout::Split).cost, "{1}");
        assert_eq!(seeded(CardLayout::Flip).cost, "");
        let meld = with_layout(&text, CardLayout::Meld).unwrap().text;
        assert_eq!(Form::of_side(&meld, Side::Third).unwrap().cost, "");
        assert!(Form::of(&meld)
            .unwrap()
            .rules
            .contains("meld them into Melded Card"));
    }

    #[test]
    fn back_to_one_face_drops_the_part_and_leaves_the_file_as_it_was() {
        let text = format!(
            "# my drone\n{}",
            DRONE.replace("flavor", "# the flavour\nflavor")
        );
        for to in CardLayout::ALL {
            let there = with_layout(&text, to).unwrap().text;
            let back = with_layout(&there, CardLayout::One).unwrap();
            assert_eq!(back.text, text, "{to:?}");
            assert_eq!(
                back.lost.len(),
                match to {
                    CardLayout::One => 0,
                    CardLayout::Meld => 2,
                    _ => 1,
                }
            );
        }
        let one = with_layout(SHINOVAR, CardLayout::One).unwrap();
        assert_eq!(one.lost, ["\"Kaladin, Herald of Second Chances\""]);
        assert!(!one.text.contains("layout") && !one.text.contains("[back]"));
        checks(&one.text).unwrap();
    }

    #[test]
    fn transform_and_modal_trade_only_the_layout_line() {
        let modal = with_layout(SHINOVAR, CardLayout::Modal).unwrap();
        assert!(modal.lost.is_empty());
        assert_eq!(
            modal.text,
            SHINOVAR.replace("layout = \"transform\"", "layout = \"modal\"")
        );
        let transform = with_layout(&modal.text, CardLayout::Transform).unwrap();
        assert_eq!(transform.text, SHINOVAR);
    }

    #[test]
    fn a_part_moved_keeps_its_comments_and_what_still_fits() {
        let text = SHINOVAR
            .replace("[back]", "# the herald\n[back]")
            .replace("pt = \"4/4\"", "pt = \"4/4\" # big");
        for to in [CardLayout::Split, CardLayout::Flip, CardLayout::Meld] {
            let relaid = with_layout(&text, to).unwrap();
            let key = to.second().unwrap();
            assert!(
                relaid.text.contains(&format!("# the herald\n[{key}]")),
                "{}",
                relaid.text
            );
            assert!(relaid.text.contains("pt = \"4/4\" # big"));
            let second = Form::of_side(&relaid.text, Side::Second).unwrap();
            assert_eq!(second.name, "Kaladin, Herald of Second Chances");
            assert!(second.rules.starts_with("Flying, vigilance"));
        }
        // A flipped half shares the upright half's colour and art.
        let flip = with_layout(&text, CardLayout::Flip).unwrap();
        assert_eq!(
            flip.lost,
            [
                "\"Kaladin, Herald of Second Chances\"'s color_indicator",
                "\"Kaladin, Herald of Second Chances\"'s artist"
            ]
        );
    }

    #[test]
    fn a_back_made_a_spell_loses_what_no_spell_has_and_says_so() {
        let relaid = with_layout(SHINOVAR, CardLayout::Adventure).unwrap();
        let spell = Form::of_side(&relaid.text, Side::Second).unwrap();
        assert_eq!(spell.type_line, "Sorcery — Adventure");
        assert_eq!(spell.corner, FormCorner::None);
        assert_eq!(spell.cost, "{1}");
        assert_eq!(
            spell.flavor,
            Form::of_side(SHINOVAR, Side::Second).unwrap().flavor
        );
        let kaladin = "\"Kaladin, Herald of Second Chances\"";
        assert_eq!(
            relaid.lost,
            [
                format!("{kaladin}'s pt"),
                format!("{kaladin}'s artist"),
                format!("{kaladin}'s color_indicator"),
                format!("{kaladin}'s type \"Legendary Enchantment Creature — Human Knight\""),
            ]
        );
    }

    #[test]
    fn an_inset_spell_changes_only_its_subtype_between_insets() {
        let omen = with_layout(SKYBREAKER, CardLayout::Omen).unwrap();
        assert!(omen.lost.is_empty());
        assert_eq!(
            omen.text,
            SKYBREAKER.replace("Instant — Adventure", "Instant — Omen")
        );
        checks(&omen.text).unwrap();
        let prepare = with_layout(&omen.text, CardLayout::Prepare).unwrap();
        assert_eq!(
            Form::of_side(&prepare.text, Side::Second)
                .unwrap()
                .type_line,
            "Instant"
        );
        assert!(prepare.text.contains("layout = \"prepare\""));
        checks(&prepare.text).unwrap();
        let adventure = with_layout(&prepare.text, CardLayout::Adventure).unwrap();
        assert_eq!(adventure.text, SKYBREAKER);
        // Out of the text box it is no Adventure.
        let transform = with_layout(SKYBREAKER, CardLayout::Transform).unwrap();
        assert_eq!(
            Form::of_side(&transform.text, Side::Second)
                .unwrap()
                .type_line,
            "Instant"
        );
        assert!(transform.lost.is_empty());
    }

    #[test]
    fn a_meld_pair_laid_out_as_two_parts_drops_the_melded_card() {
        let relaid = with_layout(SMUGGLER, CardLayout::Transform).unwrap();
        assert_eq!(relaid.lost, ["\"The Kessel Runner\""]);
        assert_eq!(
            Form::of_side(&relaid.text, Side::Second).unwrap(),
            Form::of_side(SMUGGLER, Side::Second).unwrap()
        );
        checks(&relaid.text).unwrap();
        // Back to a pair, its card already melds them: its text is left be.
        let meld = with_layout(SHINOVAR, CardLayout::Meld).unwrap();
        let again = with_layout(&meld.text, CardLayout::Meld).unwrap();
        assert_eq!(again.text, meld.text);
        // Made a pair again, it melds into a melded card made anew.
        let smuggler = with_layout(&relaid.text, CardLayout::Meld).unwrap();
        assert_eq!(
            Form::of_side(&smuggler.text, Side::Third).unwrap().name,
            "Melded Card"
        );
        checks(&smuggler.text).unwrap();
        // The line the pair was given goes with it; the card's own stays.
        let unmelded = with_layout(&smuggler.text, CardLayout::One).unwrap();
        assert_eq!(
            Form::of(&unmelded.text).unwrap(),
            Form::of(SMUGGLER).unwrap()
        );
    }

    proptest! {
        // Whatever is typed into the form is what the file then says, on
        // either face of a card of any layout, and the other face is left
        // as it was.
        #[test]
        fn what_the_form_writes_reads_back(
            name in "\\PC{0,20}",
            rules in "(\\PC|\n|\t|\"|\\\\){0,60}",
            flavor in "(\\PC|\n|\"){0,30}",
            power in "[0-9*+-]{0,2}",
            toughness in "[0-9*+-]{0,2}",
            color_indicator in "[WUBRG]{0,2}",
            (file, side) in prop_oneof![
                Just((DRONE, Side::Front)),
                Just((SHINOVAR, Side::Front)),
                Just((SHINOVAR, Side::Second)),
                Just((FORCE, Side::Second)),
                Just((CHASM, Side::Front)),
                Just((CHASM, Side::Second)),
                Just((DETENTION, Side::Front)),
                Just((DETENTION, Side::Second)),
                Just((SKYBREAKER, Side::Front)),
                Just((SKYBREAKER, Side::Second)),
                Just((FABRIALIST, Side::Front)),
                Just((FABRIALIST, Side::Second)),
                Just((SQUIRE, Side::Front)),
                Just((SQUIRE, Side::Second)),
                Just((SMUGGLER, Side::Front)),
                Just((SMUGGLER, Side::Second)),
                Just((SMUGGLER, Side::Third)),
            ],
        ) {
            let form = Form {
                name,
                cost: "{2}{G}".into(),
                type_line: "Creature — Elf".into(),
                rules,
                flavor,
                corner: FormCorner::Pt { power, toughness },
                color_indicator,
                art: String::new(),
                artist: String::new(),
            };
            let written = form.write_side(file, side).unwrap();
            prop_assert_eq!(Form::of_side(&written, side).unwrap(), form);
            for other in [Side::Front, Side::Second, Side::Third] {
                if other != side && Form::of_side(file, other).is_ok() {
                    prop_assert_eq!(
                        Form::of_side(&written, other).unwrap(),
                        Form::of_side(file, other).unwrap()
                    );
                }
            }
        }
    }
}
