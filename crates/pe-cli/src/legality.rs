//! Legality as a question about a whole decklist.
//!
//! `pe_scryfall::legality` answers what one card decides on its own. Everything
//! here needs the list as well: how many copies there are, which cards were
//! nominated as commanders, and how many cards there are altogether. That is
//! the same seam as everywhere else in this binary — card data on one side,
//! decklist data on the other, joined here.
//!
//! Two rules govern every check below, and both exist because the failure this
//! feature could most easily introduce is a checker that is confidently wrong.
//!
//! **Silence is never evidence.** `may_be_played_in_commander` and
//! `may_appear_any_number_of_times` return `Option<bool>` precisely so a field
//! the index never carried cannot be read as "illegal". A `None` produces no
//! complaint at all. A checker crying wolf over absent data is this project's
//! own defining failure mode wearing a checker's clothes, and it is worse than
//! not checking, because a report full of imaginary violations trains its
//! reader to ignore the real one.
//!
//! **An entry that is not the card is not evidence about the card.** The index
//! is keyed by lowercased card name and Scryfall prints tokens that share a
//! name with a real card, so a lookup for `llanowar elves` can return a token's
//! data — mana value 0, no colour identity, `not_legal` in every format. Those
//! fields describe the token, not the card the decklist named, and repeating
//! them as facts about the card is the same confidently wrong claim as reading
//! a missing field as guilt. So a token-typed entry is unknown for every rule
//! that consults card data. See issue #38: the defect is in how the index is
//! built, and this crate can only decline to launder it.
//!
//! **Nothing here fails a run.** Someone brewing wants the numbers before the
//! deck is legal, and a half-built list is the normal case rather than the
//! error case. What a warning must not do is hide: it goes to stderr above the
//! results, it is counted again in the verdict, and it appears in the JSON as
//! `legality` so a caller that does want to fail on it can.
//!
//! One rule is modelled loosely on purpose. A Background is reported as a legal
//! commander whether or not the list also nominates a "Choose a Background"
//! partner for it, because whether that partner is there is a rule this does
//! not model, and guessing at it would be a violation invented from nothing.

use facet::Facet;
use pe_scryfall::legality::CommanderLegality;
use pe_scryfall::Colors;

use crate::library::Library;

/// One thing wrong with the list, as both a token and a sentence.
///
/// The token is what a CI caller matches on and the sentence is what a human
/// reads; they are the same object so the two cannot describe different
/// problems. Reported rather than fatal, for the reason in the module docs.
#[derive(Facet)]
pub struct Violation {
    /// Which rule: `commander`, `color_identity`, `singleton`,
    /// `format_legality` or `deck_size`.
    pub rule: &'static str,
    /// The card at fault, or `null` for a rule about the list as a whole.
    pub card: Option<String>,
    pub detail: String,
}

/// Everything wrong with the list, or nothing when there is nothing to say.
///
/// # Which format this is
///
/// There is no `--format` flag, so the format has to be inferred, and the only
/// evidence in the file is whether anything was nominated as a commander.
/// A list that nominates one is a Commander list and gets every check below.
///
/// A list that nominates none gets none of them, which is a deliberate refusal
/// rather than an oversight. Such a list could be a 60-card constructed deck —
/// where four copies of a card are correct, the deck is 60 rather than 100 and
/// the colour identity rule does not exist at all — or it could be a Commander
/// list whose export lost the `[Commander]` bracket. Nothing in the file tells
/// them apart, and every check here would answer differently for the two. So
/// this says nothing at all rather than picking one and being confidently
/// wrong about every card in the list, which is the same reasoning that makes a
/// missing index field produce silence rather than a violation.
pub fn check(library: &Library) -> Vec<Violation> {
    let mut found = Vec::new();
    if library.commanders.is_empty() {
        return found;
    }
    check_commanders(library, &mut found);
    check_color_identity(library, &mut found);
    check_singleton(library, &mut found);
    check_format_legality(library, &mut found);
    check_deck_size(library, &mut found);
    found
}

/// Whether each nominated commander can actually command.
fn check_commanders(library: &Library, found: &mut Vec<Violation>) {
    for entry in &library.commanders {
        // Unlike the flag-based rules, this one is derived from the type line
        // and oracle text, so an index entry carrying neither would come back
        // "cannot command" from having been asked nothing at all.
        if entry.card.type_line.is_empty() || is_token(&entry.card.type_line) {
            continue;
        }
        if entry.card.commander_route().is_some() {
            continue;
        }
        found.push(Violation {
            rule: "commander",
            card: Some(entry.card.name.clone()),
            detail: format!(
                "{} is nominated as a commander, but it is neither a legendary creature nor a card whose own text says it can be one",
                entry.card.name
            ),
        });
    }
}

/// Whether every library card fits inside the commanders' combined identity.
fn check_color_identity(library: &Library, found: &mut Vec<Violation>) {
    // One commander resolved to a token sinks the whole rule rather than one
    // card: the token's empty identity would silently shrink the deck's
    // colours, and every legal card outside them would be called illegal.
    if library
        .commanders
        .iter()
        .any(|e| is_token(&e.card.type_line))
    {
        return;
    }

    // Partners and a Background define the identity jointly, so the union is
    // the deck's colours and no commander is measured against another.
    let commander_ci: String = library
        .commanders
        .iter()
        .flat_map(|e| e.card.ci.iter().map(String::as_str))
        .collect();
    let allowed_letters = letters(&commander_ci);

    // A colourless commander goes unchecked. `ci` defaults to empty when the
    // index never carried the field, so an empty combined identity is
    // indistinguishable from no data — and acting on it would mean reporting
    // every coloured card in the deck as illegal on the strength of a field
    // that may simply be absent. That loses a real rule for the rarest decks in
    // the format, which is the cheaper of the two mistakes by a wide margin.
    if allowed_letters.is_empty() {
        return;
    }
    let allowed = Colors::from_letters(&allowed_letters).unwrap_or_default();

    let commanders = library.commander_names().join(" and ");
    for entry in &library.entries {
        if is_token(&entry.card.type_line) || entry.card.identity_fits_within(allowed) {
            continue;
        }
        found.push(Violation {
            rule: "color_identity",
            card: Some(entry.card.name.clone()),
            detail: format!(
                "{} has colour identity {}, which is outside {allowed_letters} — the identity of {commanders}",
                entry.card.name,
                letters(&entry.card.ci.concat()),
            ),
        });
    }
}

/// Whether any card appears more often than it is allowed to.
fn check_singleton(library: &Library, found: &mut Vec<Violation>) {
    for entry in library.played() {
        if entry.qty <= 1 || is_token(&entry.card.type_line) {
            continue;
        }
        // `Some(true)` is a basic land or one of the ten cards that lift the
        // limit outright; `None` is the index never having said, which is not
        // evidence that a second copy is wrong.
        if entry.card.may_appear_any_number_of_times() != Some(false) {
            continue;
        }
        found.push(Violation {
            rule: "singleton",
            card: Some(entry.card.name.clone()),
            detail: format!(
                "{}x {} — Commander is singleton outside basic lands and the cards that say otherwise",
                entry.qty, entry.card.name
            ),
        });
    }
}

/// Whether any card is banned or simply not a Commander card.
fn check_format_legality(library: &Library, found: &mut Vec<Violation>) {
    for entry in library.played() {
        // A token entry carries `not_legal` for every format, which is true of
        // the token and says nothing at all about the card sharing its name.
        if is_token(&entry.card.type_line) {
            continue;
        }
        // `None` is an index that never carried the word. Only an explicit
        // refusal counts.
        if entry.card.may_be_played_in_commander() != Some(false) {
            continue;
        }
        // Kept apart because they are different mistakes: a banned card was a
        // deckbuilding choice, a not-legal one is usually an Un-set or
        // online-only printing that a text export dragged in by name.
        let verdict = match entry.card.commander_legality() {
            CommanderLegality::Banned => "is banned in Commander",
            _ => "is not legal in Commander",
        };
        found.push(Violation {
            rule: "format_legality",
            card: Some(entry.card.name.clone()),
            detail: format!("{} {verdict}", entry.card.name),
        });
    }
}

/// Whether the list is a hundred cards.
fn check_deck_size(library: &Library, found: &mut Vec<Violation>) {
    let in_command_zone: u32 = library.commanders.iter().map(|e| e.qty).sum();
    let total = library.size() + in_command_zone;
    if total == 100 {
        return;
    }
    // The one rule here that consults no card data at all, so a token-shadowed
    // entry cannot mislead it: a line in a decklist is a line whatever the index
    // thinks the card is.
    //
    // Not 99 plus commanders: with partners it is 98 plus two, and stating the
    // rule as a total is the one spelling that survives both. What was already
    // dropped from the count — companions, sideboards, sticker sheets — is
    // reported separately by the exclusion note, so a list that is short here
    // and short there gets both halves of the explanation.
    found.push(Violation {
        rule: "deck_size",
        card: None,
        detail: format!(
            "a Commander deck is 100 cards; this list has {total} ({} in the library plus {in_command_zone} in the command zone)",
            library.size()
        ),
    });
}

/// Whether this index entry is a token rather than the card that was asked for.
///
/// The index is a map keyed by lowercased card name, and Scryfall prints tokens
/// that share a name with a real card — Llanowar Elves, Mutavault, Meteorite
/// and 38 others. One of the two objects wins the key, and when the token wins
/// it, every field belongs to the token: mana value 0, no colour identity,
/// `not_legal` everywhere. None of that is a fact about the card, so callers
/// treat it as no data at all. Issue #38 tracks fixing it where it belongs,
/// which is in the tool that builds the index.
///
/// `Token` is a card type, so it is a whole word on the left of the em dash,
/// found the way `zone.rs` finds its types and for the reason given there:
/// substring matching on type lines has already produced one real bug in this
/// repository.
fn is_token(type_line: &str) -> bool {
    type_line.split("//").any(|face| {
        let types = face.split_once('—').map_or(face, |(types, _)| types);
        types
            .split_whitespace()
            .any(|w| w.eq_ignore_ascii_case("token"))
    })
}

/// Colour identity letters in printed WUBRG order, deduplicated.
///
/// Spelled out from the raw identity rather than from [`Colors`], which is a
/// bitset with no way back to letters. Empty for a colourless identity, which
/// every caller here treats as "nothing to say" rather than printing it.
fn letters(raw: &str) -> String {
    let raw = raw.to_ascii_uppercase();
    "WUBRG".chars().filter(|c| raw.contains(*c)).collect()
}
