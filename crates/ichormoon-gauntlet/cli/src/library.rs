//! Joining a decklist to card data.
//!
//! This is where the two halves meet. `chip-decklist` knows what a decklist is and
//! nothing about cards; `chip-scryfall` knows what a card is and nothing about
//! decklists. Neither depends on the other — the join lives here.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chip_scryfall::index::{Card, Index, IndexFile, KeywordVocabulary, TagVocabulary};
use chip_scryfall::OutsideLibrary;
use gauntlet_criteria::{Demand, Grouping, GroupingError, ManaSource, Palette};

/// Scryfall's oracle tag for a land that always enters tapped.
///
/// 495 cards, and — checked against the search API — it does *not* hold the
/// shocklands, the fastlands or anything else whose tapped-ness is contingent.
/// Those are [`CONDITIONAL_TAPLAND`], which is why two tags are read rather
/// than one.
pub const TAPLAND: &str = "tapland";

/// Scryfall's oracle tag for a land whose tapped-ness is a decision or a
/// condition: Hallowed Fountain, Blackcleave Cliffs, Agadeem's Awakening.
///
/// 179 cards, overlapping [`TAPLAND`] by nine oddities where both readings say
/// tapped anyway. HANDS.md hand 8 is about this tag existing: "you may pay 2
/// life, if you don't it enters tapped" is not a property of the card, and a
/// model that folded it into `otag:tapland` would be answering a question the
/// pilot gets to decide.
pub const CONDITIONAL_TAPLAND: &str = "conditional-tapland";

pub struct Entry {
    pub card: Card,
    pub categories: Vec<String>,
    pub qty: u32,
}

/// A grouping bit the caller computed itself, rather than one read off a query.
///
/// `members` is one flag per [`Library::entries`] position, so it is only
/// meaningful beside the library it was built from — which is why it is passed
/// straight into [`Library::grouping_for`] rather than stored anywhere.
pub struct Marked {
    pub label: String,
    pub members: Vec<bool>,
}

/// Whether this run has to tell lands apart by what they make.
///
/// Two cards matching the same queries are interchangeable to a criterion, and
/// a Plains and an Island are not interchangeable to the mana gate — so asking
/// a mana question splits groups that would otherwise be one, and the
/// enumeration widens to match. A run that asks no mana question does not pay
/// for that, which is what this says: the detail is a cost, and it is only
/// worth paying where somebody asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaDetail<'a> {
    /// Nothing here is a mana source. The groups are exactly the query groups.
    Ignored,
    /// Lands carry what they produce and whether they arrive tapped, and a
    /// spell the declared casting priority names carries what it costs.
    ///
    /// `castable` is one entry per [`Library::entries`] position, `None` for
    /// every card this run never casts — which is all of them in a run that
    /// declared no casting priority. Held here rather than beside this enum so
    /// that "prices spells it does not model mana for" is a state nobody can
    /// build.
    Modelled { castable: &'a [Option<Demand>] },
}

/// A listed card that never enters the library, and which type made it so.
pub struct Excluded {
    pub name: String,
    pub qty: u32,
    pub card_type: OutsideLibrary,
}

pub struct Library {
    pub entries: Vec<Entry>,
    /// The nominated commanders, as whole entries rather than names.
    ///
    /// Whole entries because a name stored beside the card it names is a pair
    /// that can disagree. They start in the command zone, so what this holds is
    /// what the library does not.
    pub commanders: Vec<Entry>,
    /// SHA-256 of the decklist file, taken here because this is where its bytes
    /// are read. Hashing a re-read of the path would answer a question about a
    /// different moment in time.
    pub deck_sha256: String,
    /// When the index was built, or `None` when it never said. Carried out of
    /// the index rather than looked up again for the same reason.
    pub index_updated_at: Option<String>,
    /// Whether the index predates fields the current queries read.
    ///
    /// Worth carrying because the failure is silent: `produces:w` against an
    /// index built before that field existed matches nothing and reports a
    /// confident 0%, which is indistinguishable from a deck with no white
    /// sources. The whole point of this tool is that those two look different.
    pub index_is_stale: bool,
    /// Which oracle tags this index carries, carried out of the header for the
    /// same reason as the date: the file is read once, here.
    ///
    /// Every `otag:` term in this run is checked against it — the criteria
    /// file's own and the effect library's alike — because an index that never
    /// fetched a tag and a deck with no card in it produce the same empty
    /// result, and only this says which one happened.
    pub index_tags: TagVocabulary,
    /// Every keyword the pool this index was built from carries, read off the
    /// header beside the tags and for the same reason: `kw:flyign` and a deck
    /// playing no flier both count zero, and only this tells them apart.
    ///
    /// An empty one is not the same fact as an empty tag list. Tags are
    /// fetched, so an index carrying none says so about itself; keywords are
    /// derived from the cards, so an index that never wrote them down is silent
    /// rather than authoritative, and refuses nothing.
    pub index_keywords: KeywordVocabulary,
    /// Kept rather than dropped: excluding a card silently is the same failure
    /// as a query that matches nothing — a confident number nobody can question.
    pub excluded: Vec<Excluded>,
}

impl Library {
    pub fn load(deck: &Path, index_path: Option<&Path>) -> Result<Self> {
        let text = std::fs::read_to_string(deck)
            .with_context(|| format!("reading decklist {}", deck.display()))?;
        let parsed = chip_decklist::parse(&text)?;

        let path = index_path
            .map(Path::to_path_buf)
            .unwrap_or_else(Index::default_path);
        let index = IndexFile::open(&path)?;
        let stale = index.is_stale();
        let index_tags = index.tag_vocabulary();
        let index_keywords = index.keyword_vocabulary();

        // Strict about unknown cards: you cannot compute a land count for a
        // card you cannot look up, so a typo here would silently skew every
        // probability.
        let unknown: Vec<&str> = parsed
            .iter()
            .filter(|e| !index.contains(&e.name))
            .map(|e| e.name.as_str())
            .collect();
        if !unknown.is_empty() {
            bail!(
                "unknown card(s): {} — rebuild the index with `gauntlet sync`",
                unknown.join(", ")
            );
        }

        // The library is what you draw from: the deck minus anything outside it
        // (companions, sideboards) and minus the commanders, which start in the
        // command zone. Getting this wrong is the classic 99-versus-100 error.
        //
        // Two separate notions of "outside", deliberately kept apart. The
        // decklist says a companion or a sideboard card is out, which is
        // decklist data; the card data says a sticker sheet or an attraction is
        // out, which the decklist cannot know and `parse` therefore never
        // claims.
        let mut entries = Vec::new();
        let mut commanders = Vec::new();
        let mut excluded = Vec::new();
        for e in &parsed {
            let entry = Entry {
                card: index.get(&e.name)?.expect("checked above"),
                categories: e.categories.iter().map(|c| c.name.clone()).collect(),
                qty: e.qty.get(),
            };
            if e.is_commander() {
                commanders.push(entry);
            } else if e.is_outside() {
                continue;
            } else if let Some(card_type) = entry.card.outside_library() {
                excluded.push(Excluded {
                    name: entry.card.name,
                    qty: entry.qty,
                    card_type,
                });
            } else {
                entries.push(entry);
            }
        }

        Ok(Library {
            entries,
            commanders,
            deck_sha256: crate::report::sha256_hex(text.as_bytes()),
            index_is_stale: stale,
            index_tags,
            index_keywords,
            index_updated_at: index.updated_at().map(str::to_owned),
            excluded,
        })
    }

    pub fn size(&self) -> u32 {
        self.entries.iter().map(|e| e.qty).sum()
    }

    pub fn commander_names(&self) -> Vec<String> {
        self.commanders
            .iter()
            .map(|e| e.card.name.clone())
            .collect()
    }

    /// Group the library by which of `queries` each card matches, plus any
    /// `marked` bits the caller worked out for itself.
    ///
    /// `marked` exists for the effect library. *Which effect applies to this
    /// card* is not a query — it is the answer to several queries and a
    /// last-wins rule — so it cannot be expressed as one, and handing the
    /// engine the raw queries instead would make it re-decide the overlap on
    /// every path. The bits land after the queries, so an index a criteria
    /// clause already holds does not move.
    pub fn grouping_for(
        &self,
        queries: &[String],
        marked: &[Marked],
        mana: ManaDetail,
    ) -> Result<Grouping> {
        let parsed: Vec<_> = queries
            .iter()
            // Formatted in rather than layered as context: this error travels
            // through a generic `DiscoveryError` whose Display shows only the
            // outermost layer, so a chained cause would be dropped — and the
            // cause is the whole message, the one that names the offending term
            // and lists what would have been accepted.
            .map(|q| chip_scryfall::parse(q).map_err(|e| anyhow::anyhow!("in query {q:?}: {e}")))
            .collect::<Result<_>>()?;

        let cards = self.entries.iter().enumerate().map(|(card, e)| {
            let view = e.card.view(&e.categories);
            let mut mask = 0u64;
            for (i, q) in parsed.iter().enumerate() {
                if q.matches(&view) {
                    mask |= 1u64 << i;
                }
            }
            for (i, m) in marked.iter().enumerate() {
                if m.members[card] {
                    mask |= 1u64 << (queries.len() + i);
                }
            }
            let source = match mana {
                ManaDetail::Ignored => ManaSource::Spell,
                // A card the priority names is a payer rather than a source,
                // and the two cannot be the same card: `resolve` refuses to
                // price a land, because a land is played rather than cast.
                ManaDetail::Modelled { castable } => match castable.get(card).copied().flatten() {
                    Some(cost) => ManaSource::Castable { cost },
                    None => mana_source(&e.card),
                },
            };
            (mask, source, e.qty)
        });

        let names = queries
            .iter()
            .cloned()
            .chain(marked.iter().map(|m| m.label.clone()))
            .collect();
        Grouping::with_mana(names, cards).map_err(|e: GroupingError| anyhow::anyhow!(e))
    }

    /// Which lands in this deck have a tapped-ness the pilot decides.
    ///
    /// Named rather than counted, because a run that assumes one of those
    /// decisions has to say which cards it assumed it about — see HANDS.md hand
    /// 8. Sorted and deduplicated so the note reads the same on every run.
    pub fn conditional_taplands(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.card.tags.iter().any(|t| t == CONDITIONAL_TAPLAND))
            .map(|e| e.card.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Cards matching `query` that are not lands.
    ///
    /// The check behind `zone = "battlefield"`: a land gets there on a land
    /// drop, which this engine models, and everything else has to be cast,
    /// which it does not.
    pub fn non_lands_matching(&self, query: &str) -> Result<Vec<String>> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| q.matches(&e.card.view(&e.categories)) && !is_land(&e.card))
            .map(|e| e.card.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        Ok(names)
    }

    /// Cards matching `query` that are not lands and that nothing in this run
    /// can put onto the battlefield.
    ///
    /// The refusal behind `zone = "battlefield"`, narrowed by exactly the two
    /// ways this walk models a spell arriving there. `delivered` is what a
    /// delayed fetch can find — Urza's Saga's third chapter puts it there —
    /// and `cast` is the `[casting]` line, whose castings the battlefield
    /// count adds in. Both only for a **permanent**: an instant or a sorcery
    /// the line casts resolves and goes to a graveyard this engine does not
    /// track, so a battlefield count of one stays refused rather than
    /// answered as if it had stayed.
    pub fn stranded_matching(
        &self,
        query: &str,
        delivered: &[&str],
        cast: &[String],
    ) -> Result<Vec<String>> {
        let parse =
            |q: &str| chip_scryfall::parse(q).map_err(|e| anyhow::anyhow!("in query {q:?}: {e}"));
        let q = parse(query)?;
        let delivered = delivered
            .iter()
            .map(|q| parse(q))
            .collect::<Result<Vec<_>>>()?;
        let cast = cast.iter().map(|q| parse(q)).collect::<Result<Vec<_>>>()?;
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| {
                let view = e.card.view(&e.categories);
                let arrives = is_permanent(&e.card)
                    && (delivered.iter().any(|d| d.matches(&view))
                        || cast.iter().any(|c| c.matches(&view)));
                q.matches(&view) && !is_land(&e.card) && !arrives
            })
            .map(|e| e.card.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        Ok(names)
    }

    /// Cards matching `query` with a land on them that no land drop plays: a
    /// transforming, meld or flip card whose land is a back face.
    ///
    /// `t:land` matches them, because Scryfall reads every face. Named apart
    /// so a refusal can say why a card with "Land" printed on it is not a
    /// land here, rather than leaving the reader to find #61.
    pub fn back_face_lands(&self, query: &str) -> Result<Vec<String>> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| {
                q.matches(&e.card.view(&e.categories))
                    && !is_land(&e.card)
                    && names(&e.card.type_line, "land")
            })
            .map(|e| e.card.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        Ok(names)
    }

    /// How many library cards matching `query` can be played as a land drop.
    ///
    /// The other half of [`Library::non_lands_matching`], and the half a
    /// land-drop preference needs: a preference picking out no land at all
    /// decides no drop in this deck, and only the card data knows that.
    pub fn lands_matching(&self, query: &str) -> Result<u32> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        Ok(self
            .entries
            .iter()
            .filter(|e| q.matches(&e.card.view(&e.categories)) && is_land(&e.card))
            .map(|e| e.qty)
            .sum())
    }

    /// Positions in [`Library::entries`] of the cards matching `query`.
    ///
    /// By position rather than by name, because the caller that wants this is
    /// building a per-entry table — what each card costs — and a name is not a
    /// key: forty of them name more than one card, and a decklist can hold the
    /// same card under two categories.
    pub fn positions_matching(&self, query: &str) -> Result<Vec<usize>> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        Ok(self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| q.matches(&e.card.view(&e.categories)))
            .map(|(i, _)| i)
            .collect())
    }

    pub fn has_lands(&self) -> bool {
        self.entries.iter().any(|e| is_land(&e.card))
    }

    /// How many library cards match a query, for the per-query breakdown.
    pub fn matching(&self, query: &str) -> Result<u32> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        Ok(self
            .entries
            .iter()
            .filter(|e| q.matches(&e.card.view(&e.categories)))
            .map(|e| e.qty)
            .sum())
    }
}

/// Whether a type line names this card type, read by word.
fn names(type_line: &str, kind: &str) -> bool {
    type_line
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(kind))
}

/// The face you play from your hand: the front, or the whole card where the
/// index carries no faces.
fn front(card: &Card) -> &str {
    card.faces
        .first()
        .map_or(card.type_line.as_str(), |f| f.type_line.as_str())
}

/// Whether this card can be put into play with a land drop.
///
/// Read off the type lines rather than from a query, because it is asked of
/// every card in the deck on every run that models mana and a parsed query per
/// card would be the expensive way to ask a one-word question.
///
/// **Not the same question as `t:land`**, and that is the whole of
/// [#61](https://github.com/cramt/progress-engine/issues/61). Scryfall's `t:`
/// reads both faces joined by `//`, which is right for *is there a land on
/// this card* and wrong for *does this card arrive on a land drop*. It does
/// when its **front** face is a land, or when it is a **modal** double-faced
/// card with a land face — you may play Jwari Disruption's back instead of
/// casting the front. A transforming, meld or flip back is reached some other
/// way: Search for Azcanta is a `{1}{U}` enchantment that becomes a land only
/// by transforming, and read off the joined line it was a free untapped blue
/// source on one game in nine.
pub fn is_land(card: &Card) -> bool {
    names(front(card), "land")
        || (card.layout == "modal_dfc" && card.faces.iter().any(|f| names(&f.type_line, "land")))
}

/// Whether a card stays on the battlefield once it resolves.
///
/// Read off the face you cast, by word: an artifact, a creature, an
/// enchantment, a planeswalker, a battle or a land. A transforming card is
/// the permanent its front face is.
pub fn is_permanent(card: &Card) -> bool {
    [
        "artifact",
        "creature",
        "enchantment",
        "planeswalker",
        "battle",
        "land",
    ]
    .iter()
    .any(|kind| names(front(card), kind))
}

/// What a card does for mana before anything is cast.
///
/// **Where HANDS.md hand 8 is decided, and it is decided pessimistically.** A
/// shockland's "you may pay 2 life" is a choice, so `otag:conditional-tapland`
/// membership settles nothing on its own — and this assumes the life is not
/// paid, so the land enters tapped and makes no mana the turn it arrives.
///
/// That understates every real shockland manabase, which is the direction this
/// tool prefers to be wrong in: a number the deck can beat is a worse failure
/// than a number it beats. It is not silent about it either — the run names
/// every card the assumption touched. Making it declarable per deck is the
/// obvious next step and is deliberately not a default nobody stated.
fn mana_source(card: &Card) -> ManaSource {
    if !is_land(card) {
        // A Sol Ring makes mana and is not here. Getting it onto the
        // battlefield costs mana, which is the budget half of #10.
        return ManaSource::Spell;
    }
    ManaSource::Land {
        enters_tapped: card
            .tags
            .iter()
            .any(|t| t == TAPLAND || t == CONDITIONAL_TAPLAND),
        produces: Palette::from_letters(&card.produces),
    }
}
