//! A mana cost in the order Oracle text writes it, so a cost built up from
//! buttons reads as a printed one does: `{X}`, the generic part, twobrid,
//! colourless, the coloured symbols in the card's colour order
//! ([`Colors::ordered`]) with a hybrid among them where its colours fall,
//! then snow, which the one printed cost holding both writes last.

use crate::card::{Color, Colors};

/// Where a symbol goes, first by kind and then, for coloured symbols, by
/// where its colours sit in the cost's colour order.
#[derive(Debug, PartialEq, PartialOrd)]
enum Place {
    Variable(usize),
    Generic,
    Twobrid(usize),
    Colorless(usize),
    /// The mean of its colours' places in the order, and a hybrid before a
    /// single colour at the same place: Bant's `{G/U}{W}`, Jund's `{B/G}{R}`.
    Colored(f64, std::cmp::Reverse<usize>),
    Snow,
}

fn place(symbol: &str, order: &[Color]) -> Place {
    let at = |c: &Color| order.iter().position(|o| o == c).unwrap_or(0);
    if let Some(i) = ["X", "Y", "Z"].iter().position(|v| *v == symbol) {
        return Place::Variable(i);
    }
    if !symbol.is_empty() && symbol.bytes().all(|b| b.is_ascii_digit()) {
        return Place::Generic;
    }
    let colors: Vec<Color> = symbol.split('/').filter_map(single_color).collect();
    if let Some(c) = symbol.strip_prefix("2/").and_then(single_color) {
        return Place::Twobrid(at(&c));
    }
    if symbol == "S" {
        return Place::Snow;
    }
    if colors.is_empty() {
        return Place::Colorless(usize::from(symbol != "C"));
    }
    let mean = colors.iter().map(at).sum::<usize>() as f64 / colors.len() as f64;
    Place::Colored(mean, std::cmp::Reverse(colors.len()))
}

fn single_color(part: &str) -> Option<Color> {
    let mut chars = part.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Color::of_letter(c),
        _ => None,
    }
}

/// `cost`'s symbols in Oracle text's order. Text outside braces is dropped,
/// and symbols of one place keep the order they came in.
pub fn in_order(cost: &str) -> String {
    let symbols: Vec<&str> = cost
        .split('{')
        .skip(1)
        .filter_map(|s| s.split_once('}').map(|(sym, _)| sym))
        .collect();
    let colors = symbols.iter().fold(Colors::default(), |mut all, s| {
        for c in s.split('/').filter_map(single_color) {
            all.insert(c);
        }
        all
    });
    let order = colors.ordered();
    let mut placed: Vec<(Place, &str)> = symbols.iter().map(|s| (place(s, &order), *s)).collect();
    placed.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("places are never NaN"));
    placed.iter().map(|(_, s)| format!("{{{s}}}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cost_reads_as_oracle_writes_it() {
        for (built, printed) in [
            ("{W}{2}", "{2}{W}"),
            ("{2}{X}{R}", "{X}{2}{R}"),
            ("{2}{W}{U}{R}{S}{R}{R}", "{2}{U}{R}{R}{R}{W}{S}"),
            ("{W}{R}{G}", "{R}{G}{W}"),
            ("{W}{U}{R}{1}", "{1}{U}{R}{W}"),
            ("{U}{G}{2}", "{2}{G}{U}"),
            ("{R}{B/G}", "{B/G}{R}"),
            ("{W}{G/U}", "{G/U}{W}"),
            ("{W}{G/W/P}{G}{1}{G}", "{1}{G}{G}{G/W/P}{W}"),
            ("{2/W}{2/R}{2/U}", "{2/U}{2/R}{2/W}"),
            ("{U}{C}{8}{C}", "{8}{C}{C}{U}"),
            ("", ""),
        ] {
            assert_eq!(in_order(built), printed, "{built}");
        }
    }

    /// Against every card in the committed index: the order is Oracle's for
    /// all but a few odd costs (Un-cards, a five-hybrid cost, old printings
    /// that wrote `{U}{G}`).
    #[test]
    fn real_costs_are_already_in_order() {
        let index = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../decks/index.jsonl");
        let text = std::fs::read_to_string(index).expect("decks/index.jsonl");
        let mut costs = std::collections::BTreeSet::new();
        for line in text.lines().skip(1) {
            let Some((_, json)) = line.split_once('\t') else {
                continue;
            };
            let card: serde_json::Value = serde_json::from_str(json).expect("an index record");
            let faces = card["faces"].as_array().cloned().unwrap_or_default();
            for cost in faces
                .iter()
                .chain([&card])
                .filter_map(|f| f["mana_cost"].as_str())
            {
                for part in cost.split(" // ").filter(|p| !p.is_empty()) {
                    costs.insert(part.to_string());
                }
            }
        }
        let wrong: Vec<&String> = costs.iter().filter(|c| in_order(c) != **c).collect();
        assert!(costs.len() > 800, "only {} costs read", costs.len());
        assert!(
            wrong.len() * 50 < costs.len(),
            "{} of {} out of order: {wrong:?}",
            wrong.len(),
            costs.len()
        );
    }
}
