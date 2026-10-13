//! The corpus: every card in Scryfall's oracle bulk file that Vintage
//! plays, face by face, as `scryfall sync` keeps it in `~/.cache/scryfall/`.

use std::collections::HashSet;
use std::fs;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::golden::Entry;

/// Layouts that are not cards a deck can hold.
const NOT_CARDS: &[&str] = &[
    "token",
    "double_faced_token",
    "emblem",
    "art_series",
    "scheme",
    "planar",
    "vanguard",
    "reversible_card",
];

/// Cards with a rule in the Comprehensive Rules written for them alone. They
/// are the tail a grammar should not chase: each is its own special case in
/// any engine, so the headline leaves them out and says how many.
pub const EXCLUDED_TAG: &str = "unique-cr-reference";

/// The scope is paper Magic: every card legal or restricted in Vintage.
/// Vintage takes every paper set and bans only on principle, never for
/// power, so this leaves out exactly the digital-only cards (Alchemy's
/// "perpetually"), the Un-cards, and the cards no paper game plays: ante,
/// dexterity (Chaos Orb), subgames (Shahrazad), Conspiracy's draft cards,
/// and the cards banned in 2020 for racist content.
fn vintage_playable(c: &Value) -> bool {
    matches!(
        c["legalities"]["vintage"].as_str(),
        Some("legal" | "restricted")
    )
}

pub struct Printed {
    pub name: String,
    pub edhrec_rank: Option<u64>,
    /// Each face as the bulk file has it.
    pub faces: Vec<Entry>,
}

pub fn cache(file: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.cache/scryfall/{file}")
}

pub fn excluded_ids(tags_path: &str) -> Result<HashSet<String>> {
    let text = fs::read_to_string(tags_path).with_context(|| format!("reading {tags_path}"))?;
    for line in text.lines() {
        let v: Value = serde_json::from_str(line)?;
        if v["slug"] == EXCLUDED_TAG {
            return Ok(v["taggings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["oracle_id"].as_str().map(str::to_string))
                .collect());
        }
    }
    bail!("{tags_path} has no {EXCLUDED_TAG} tag")
}

/// The cards of a bulk file, less the excluded ones (and how many those
/// were), or only the one named `only`.
pub fn read(
    bulk: &str,
    excluded: &HashSet<String>,
    only: Option<&str>,
) -> Result<(Vec<Printed>, usize)> {
    let text = fs::read_to_string(bulk).with_context(|| format!("reading {bulk}"))?;
    let mut cards = Vec::new();
    let mut skipped = 0;
    for line in text.lines() {
        let c: Value = serde_json::from_str(line)?;
        let layout = c["layout"].as_str().unwrap_or("");
        if NOT_CARDS.contains(&layout) || !vintage_playable(&c) {
            continue;
        }
        if c["oracle_id"]
            .as_str()
            .is_some_and(|id| excluded.contains(id))
        {
            skipped += 1;
            continue;
        }
        let name = c["name"].as_str().unwrap_or("").to_string();
        if only.is_some_and(|o| !name.eq_ignore_ascii_case(o)) {
            continue;
        }
        let face = |f: &Value, n: &str| Entry {
            card: name.clone(),
            face: n.to_string(),
            type_line: f["type_line"].as_str().unwrap_or("").to_string(),
            text: f["oracle_text"].as_str().unwrap_or("").to_string(),
            mana_cost: f["mana_cost"].as_str().unwrap_or("").to_string(),
        };
        let faces: Vec<Entry> = match c["card_faces"].as_array() {
            Some(fs) if c["oracle_text"].is_null() => fs
                .iter()
                .map(|f| face(f, f["name"].as_str().unwrap_or("")))
                .collect(),
            _ => vec![face(&c, &name)],
        };
        cards.push(Printed {
            name,
            edhrec_rank: c["edhrec_rank"].as_u64(),
            faces,
        });
    }
    Ok((cards, skipped))
}
