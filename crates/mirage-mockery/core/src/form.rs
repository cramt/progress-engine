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

use anyhow::{bail, Result};
use facet::Facet;
use toml_edit::{DocumentMut, Item, Value};

use crate::card::RawCard;

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
        // facet_toml reads `type = ` with nothing after it as empty, and the
        // form must be able to write back what it opened.
        text.parse::<DocumentMut>()?;
        let raw = RawCard::parse(text)?;
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
        let mut doc: DocumentMut = text.parse()?;
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
            set(&mut doc, key, want)?;
        }
        Ok(doc.to_string())
    }
}

fn set(doc: &mut DocumentMut, key: &str, want: Option<String>) -> Result<()> {
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

    proptest! {
        // Whatever is typed into the form is what the file then says.
        #[test]
        fn what_the_form_writes_reads_back(
            name in "\\PC{0,20}",
            rules in "(\\PC|\n|\t|\"|\\\\){0,60}",
            flavor in "(\\PC|\n|\"){0,30}",
            power in "[0-9*+-]{0,2}",
            toughness in "[0-9*+-]{0,2}",
            color_indicator in "[WUBRG]{0,2}",
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
            let written = form.write_into(DRONE).unwrap();
            prop_assert_eq!(Form::of(&written).unwrap(), form);
        }
    }
}
