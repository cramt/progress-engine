//! Joining a decklist to card data.
//!
//! This is where the two halves meet. `chip-decklist` knows what a decklist is and
//! nothing about cards; `chip-scryfall` knows what a card is and nothing about
//! decklists. Neither depends on the other — the join lives here.

use std::path::Path;

use anyhow::{bail, Context, Result};
use chip_decklist::deck::{self, CardRef, Deck};
use chip_scryfall::index::{Card, Index, IndexFile, KeywordVocabulary, TagVocabulary};
use chip_scryfall::OutsideLibrary;
use gauntlet_criteria::{Demand, Grouping, GroupingError, ManaSource, Palette, Resolves};

use crate::lands::{self, Reading};

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
/// `members` is one flag per [`Library::entries`] position and then one per
/// [`Library::commanders`] position, so it is only
/// meaningful beside the library it was built from — which is why it is passed
/// straight into [`Library::grouping_for`] rather than stored anywhere.
pub struct Marked {
    pub label: String,
    pub members: Vec<bool>,
}

/// What a rock or dork adds once the `[casting]` line has cast it, as the
/// effect that owns the card declares it (ADR-0018).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Adds {
    /// How many mana a turn.
    pub amount: u32,
    /// Whole turns it waits after the one it is cast on: `after = n`, which
    /// is how a rock that enters tapped is declared. Zero for most.
    pub after: u32,
}

/// Keywords that let a spell be cast for less than its printed cost, which
/// the budget does not model: a spell with one pays what it prints.
const COST_REDUCERS: [&str; 3] = ["Improvise", "Affinity", "Convoke"];

/// What [`Library::line_mana`] found.
#[derive(Debug, Default)]
pub struct LineMana {
    /// Each rock or dork the line names, by card, and what it is to the budget.
    pub sources: Vec<(String, ManaSource)>,
    /// Cards the line names that could make mana and are counted as none.
    pub uncounted: Vec<String>,
    /// Cards the line names whose cost a keyword could reduce.
    pub printed_cost: Vec<String>,
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
    ///
    /// `commanders` is the same table for [`Library::commanders`]: what each
    /// one costs where the line names it. A commander the line names is cast
    /// from the command zone; one it does not is left out of the grouping
    /// altogether, because it is never drawn and nothing else reads it.
    ///
    /// `adds` is one entry per [`Library::entries`] position too: what each
    /// card adds where an effect declares it. A card the line casts that adds
    /// something is a rock or a dork; one it never casts makes no mana.
    Modelled {
        castable: &'a [Option<Demand>],
        commanders: &'a [Option<Demand>],
        adds: &'a [Option<Adds>],
    },
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
    /// What each entry makes for mana, parallel to `entries`, read once from
    /// its card and from the deck around it — a fetchland is the lands it can
    /// find, so it cannot be read from its own card alone.
    mana: Vec<ManaSource>,
    /// Every land read as something other than its card data's face value,
    /// and how. Named by any run that prices mana.
    mana_readings: Vec<Reading>,
    /// The conditional taplands the file declared enter untapped, by name:
    /// read as untapped, and out of [`Library::conditional_taplands`],
    /// because the run no longer assumes anything about them.
    pub declared_untapped: Vec<String>,
    /// Cards counted in the library from a category whose name says tokens.
    /// Each resolved to a real card that shares a token's name, which is right
    /// if the list meant the card and wrong if it meant the token, and only
    /// the owner of the list knows which.
    pub token_named: Vec<String>,
}

impl Library {
    pub fn load(deck: &Path, index_path: Option<&Path>) -> Result<Self> {
        let text = std::fs::read_to_string(deck)
            .with_context(|| format!("reading decklist {}", deck.display()))?;
        let parsed = read_deck(deck, &text)?;

        let path = index_path
            .map(Path::to_path_buf)
            .unwrap_or_else(Index::default_path);
        let index = IndexFile::open(&path)?;
        let stale = index.is_stale();
        let index_tags = index.tag_vocabulary();
        let index_keywords = index.keyword_vocabulary();

        // Outside the deck by the deck's own say-so — a category typed
        // not-in-deck, or Archidekt's `{noDeck}`, sideboard, maybeboard or
        // companion — and never looked up. Such a line moves no probability,
        // so failing to resolve it is an error about something declared
        // irrelevant, and a token under `{noDeck}` is exactly that (#51). A
        // commander is in the command zone, which is in the deck, and is
        // still resolved.
        let counted = resolve(parsed.cards.iter().filter(|c| c.in_deck()), &index, &path)?;
        let counted: Vec<&Line> = counted.iter().collect();

        // Strict about unknown cards: you cannot compute a land count for a
        // card you cannot look up, so a typo here would silently skew every
        // probability.
        let unknown: Vec<&Line> = counted
            .iter()
            .copied()
            .filter(|e| !index.contains(&e.name))
            .collect();
        // And a name the index does hold, as a token's blank helper record
        // rather than a card: an index built before `sync` knew to leave those
        // out keys 273 of them, under names like Treasure and Spirit. Counted,
        // one would be a card with no type in the library.
        let mut not_cards = Vec::new();
        for e in counted.iter().copied() {
            if index.get(&e.name)?.is_some_and(|card| !card.is_a_card()) {
                not_cards.push(e);
            }
        }
        if !unknown.is_empty() || !not_cards.is_empty() {
            bail!("{}", unknown_cards(&unknown, &not_cards));
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
        let mut token_named = Vec::new();
        for e in counted {
            let entry = Entry {
                card: index.get(&e.name)?.expect("checked above"),
                categories: e.categories.clone(),
                qty: e.qty,
            };
            if e.commander {
                commanders.push(entry);
            } else if let Some(card_type) = entry.card.outside_library() {
                excluded.push(Excluded {
                    name: entry.card.name,
                    qty: entry.qty,
                    card_type,
                });
            } else {
                // Counted, and filed under a category that says token. Treasure,
                // Spirit and Shapeshifter are tokens and also real cards, and
                // the index only holds the card — so this line resolved to a
                // card the list may have meant as a token, and is in the
                // library. Said out loud rather than decided (#51).
                if e.categories.iter().any(|c| looks_like_tokens(c)) {
                    token_named.push(entry.card.name.clone());
                }
                entries.push(entry);
            }
        }

        let (mana, mana_readings) = lands::read(&entries, &[]);
        Ok(Library {
            mana,
            mana_readings,
            declared_untapped: Vec::new(),
            entries,
            commanders,
            token_named,
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
                ManaDetail::Modelled { castable, adds, .. } => {
                    let adds = adds.get(card).copied().flatten();
                    let cost = castable.get(card).copied().flatten();
                    let rock = cost
                        .zip(adds)
                        .and_then(|(c, a)| self.rock_or_dork(&e.card, c, a));
                    match cost {
                        Some(_) if rock.is_some() => rock.expect("just checked"),
                        Some(cost) => ManaSource::Castable {
                            cost,
                            resolves: if is_permanent(&e.card) {
                                Resolves::OntoBattlefield
                            } else {
                                Resolves::IntoGraveyard
                            },
                        },
                        None => self.mana[card],
                    }
                }
            };
            (mask, source, e.qty)
        });

        let names = queries
            .iter()
            .cloned()
            .chain(marked.iter().map(|m| m.label.clone()))
            .collect();
        let grouping =
            Grouping::with_mana(names, cards).map_err(|e: GroupingError| anyhow::anyhow!(e))?;
        let ManaDetail::Modelled { commanders, .. } = mana else {
            return Ok(grouping);
        };
        // The commanders the line casts, from the command zone. They carry the
        // criteria file's query bits like any card, and the bit of the effect
        // that owns them: Borborygmos and Fblthp draws and discards as it
        // enters, which is when the line casts it.
        let command = self
            .commanders
            .iter()
            .enumerate()
            .zip(commanders)
            .filter_map(|((at, e), cost)| {
                let cost = (*cost)?;
                let view = e.card.view(&e.categories);
                let mut mask = parsed
                    .iter()
                    .enumerate()
                    .filter(|(_, q)| q.matches(&view))
                    .fold(0u64, |mask, (i, _)| mask | 1u64 << i);
                for (i, m) in marked.iter().enumerate() {
                    if m.members.get(self.entries.len() + at) == Some(&true) {
                        mask |= 1u64 << (queries.len() + i);
                    }
                }
                // A commander is a creature, or at least a permanent, so it
                // resolves onto the battlefield; read off the card anyway.
                let resolves = if is_permanent(&e.card) {
                    Resolves::OntoBattlefield
                } else {
                    Resolves::IntoGraveyard
                };
                Some((mask, ManaSource::Castable { cost, resolves }, e.qty))
            })
            .collect::<Vec<_>>();
        Ok(grouping.with_command_zone(command))
    }

    /// A card the line casts, and what it adds once it has.
    ///
    /// The palette is the card's `produces`, except where its mana is "any
    /// color in your commander's color identity" (Arcane Signet): that is the
    /// commanders' identity, and none at all in a deck with no commander,
    /// because then there is no identity to make a colour of. A creature is
    /// summoning-sick (CR 302.6), so it waits at least the turn it is cast.
    ///
    /// `None` where it is no source after all: an instant or a sorcery is not
    /// in play to tap, and a card whose palette comes to nothing makes no mana
    /// — which is not the same as mana of no colour, because that would still
    /// pay generic.
    pub fn rock_or_dork(&self, card: &Card, cost: Demand, adds: Adds) -> Option<ManaSource> {
        let mut makes = Palette::from_letters(&card.produces);
        if card.oracle.contains("commander's color identity") {
            makes = makes.intersect(Palette::from_letters(
                self.commanders.iter().flat_map(|c| c.card.ci.iter()),
            ));
        }
        if makes.is_empty() || !is_permanent(card) {
            return None;
        }
        let sick = u32::from(names(front(card), "creature"));
        Some(ManaSource::RockOrDork {
            cost,
            adds: adds.amount,
            makes,
            waits: adds.after.max(sick),
        })
    }

    /// What the line does about mana beyond the lands: the rocks and dorks it
    /// names, as sources the budget counts, and the cards it names that could
    /// make mana in some game and are counted as making none — Fellwar Stone,
    /// which needs an opponent, and Lotus Cobra, which needs landfall — and
    /// the cards whose cost could be reduced, which pay their printed cost.
    ///
    /// Named rather than counted, because each one moves a number and the
    /// percentage cannot say so (ADR-0018). Each list is sorted and
    /// deduplicated so the note reads the same on every run.
    pub fn line_mana(
        &self,
        castable: &[Option<Demand>],
        commanders: &[Option<Demand>],
        adds: &[Option<Adds>],
    ) -> LineMana {
        let mut line = LineMana::default();
        let named = self
            .entries
            .iter()
            .zip(castable)
            .zip(adds.iter().copied().chain(std::iter::repeat(None)))
            .map(|((e, cost), adds)| (e, cost, adds))
            .chain(
                self.commanders
                    .iter()
                    .zip(commanders)
                    .map(|(e, cost)| (e, cost, None)),
            );
        for (e, cost, adds) in named {
            let Some(cost) = cost else { continue };
            let card = &e.card;
            match adds.and_then(|a| self.rock_or_dork(card, *cost, a)) {
                Some(source) => line.sources.push((card.name.clone(), source)),
                None if !card.produces.is_empty() => line.uncounted.push(card.name.clone()),
                None => {}
            }
            if card
                .keywords
                .iter()
                .any(|k| COST_REDUCERS.iter().any(|r| k.eq_ignore_ascii_case(r)))
            {
                line.printed_cost.push(card.name.clone());
            }
        }
        line.sources.sort_by(|a, b| a.0.cmp(&b.0));
        line.sources.dedup_by(|a, b| a.0 == b.0);
        for names in [&mut line.uncounted, &mut line.printed_cost] {
            names.sort_unstable();
            names.dedup();
        }
        line
    }

    /// Which cards in this library have dredge, sorted and deduplicated.
    ///
    /// The engine never dredges (ADR-0017): a run whose line can put one of
    /// these in the graveyard plays a line the pilot could have played, and
    /// says so by naming them.
    pub fn dredgers(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| e.card.keywords.iter().any(|k| k == "Dredge"))
            .map(|e| e.card.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Read the conditional taplands `queries` match as entering untapped,
    /// as the file's `[assume] untapped` declares, and say which they were.
    ///
    /// Only a conditional tapland: one Scryfall tags as always tapped has no
    /// condition for a declaration to settle, so naming one is refused rather
    /// than read as untapped, which no game would let it be. A query matching
    /// no conditional tapland comes back in the second list, to be noted.
    pub fn declare_untapped(
        &mut self,
        queries: &[String],
        file: &str,
    ) -> Result<Vec<String>, crate::refusal::Refusal> {
        let mut unmatched = Vec::new();
        for query in queries {
            crate::refusal::check_query(file, crate::refusal::QuerySite::Assume, query, self)?;
            let q = chip_scryfall::parse(query).expect("checked above");
            let lands = self
                .entries
                .iter()
                .filter(|e| is_land(&e.card) && q.matches(&e.card.view(&e.categories)));
            let mut always = Vec::new();
            let mut matched = false;
            for e in lands {
                let tagged = |tag: &str| e.card.tags.iter().any(|t| t == tag);
                if tagged(TAPLAND) {
                    always.push(e.card.name.clone());
                } else if tagged(CONDITIONAL_TAPLAND) {
                    matched = true;
                    self.declared_untapped.push(e.card.name.clone());
                }
            }
            if !always.is_empty() {
                always.sort_unstable();
                always.dedup();
                return Err(crate::refusal::Refusal::UntappedAlwaysTapped {
                    file: file.to_string(),
                    query: query.clone(),
                    lands: always,
                });
            }
            if !matched {
                unmatched.push(query.clone());
            }
        }
        self.declared_untapped.sort_unstable();
        self.declared_untapped.dedup();
        let (mana, readings) = lands::read(&self.entries, &self.declared_untapped);
        self.mana = mana;
        self.mana_readings = readings;
        Ok(unmatched)
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
            .filter(|e| !self.declared_untapped.contains(&e.card.name))
            .map(|e| e.card.name.clone())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Every land this run reads as something other than its card data says,
    /// and how: a fetchland as the lands it can find, Maze of Ith as no mana,
    /// Castle Doom as `{C}`. Sorted by card, so the note reads the same on
    /// every run.
    pub fn mana_readings(&self) -> &[Reading] {
        &self.mana_readings
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

    /// Cards matching `query` that are not permanents: instants and sorceries,
    /// which nothing can put onto the battlefield (CR 110.4b).
    pub fn non_permanents_matching(&self, query: &str) -> Result<Vec<String>> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        let mut names: Vec<String> = self
            .entries
            .iter()
            .filter(|e| q.matches(&e.card.view(&e.categories)) && !is_permanent(&e.card))
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
    /// the line casts resolves into the graveyard, where a graveyard clause
    /// counts it, so a battlefield count of one stays refused rather than
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
        Self::positions_in(&self.entries, query)
    }

    /// The same, in [`Library::commanders`]: which of the commanders `query`
    /// picks out.
    pub fn commanders_matching(&self, query: &str) -> Result<Vec<usize>> {
        Self::positions_in(&self.commanders, query)
    }

    fn positions_in(entries: &[Entry], query: &str) -> Result<Vec<usize>> {
        let q =
            chip_scryfall::parse(query).map_err(|e| anyhow::anyhow!("in query {query:?}: {e}"))?;
        Ok(entries
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

/// Whether a decklist category holds tokens rather than cards: Archidekt's
/// `Tokens & Extras`, or anything else a person named for them.
fn looks_like_tokens(category: &str) -> bool {
    category.to_ascii_lowercase().contains("token")
}

/// Why a decklist could not be resolved, saying what would actually help.
///
/// `sync` only helps a real card newer than the index. It never helps a token,
/// because the index deliberately holds no tokens — so a line filed under a
/// token category is told the remedy that works for it (#51).
/// A deck line with its card named, whichever way the file named it.
struct Line {
    name: String,
    qty: u32,
    categories: Vec<String>,
    commander: bool,
}

/// Reads a deck in either format the family knows: its own `.deck.toml`
/// (ADR-0020), or Archidekt's text export, which imports into the same `Deck`
/// and puts every card where this tool always has.
pub fn read_deck(path: &Path, text: &str) -> Result<Deck> {
    let deck = if path.extension().is_some_and(|e| e == "toml") {
        Deck::parse(text)?
    } else {
        Deck::from_archidekt(text)?
    };
    Ok(deck)
}

/// A deck in either format as Archidekt text, the one shape `parse` emits.
///
/// A `.deck.toml` names cards by printing only, so its names come from the
/// index at `index_path` (the default index when `None`). Going through the
/// exporter rather than a second reader keeps one definition of what a line
/// means: `commander`, `outside` and categories come out exactly as they
/// would for the same deck pasted from Archidekt.
pub fn archidekt_text(path: &Path, text: &str, index_path: Option<&Path>) -> Result<String> {
    if !path.extension().is_some_and(|e| e == "toml") {
        return Ok(text.to_owned());
    }
    let deck = Deck::parse(text)?;
    let index_path = index_path
        .map(Path::to_path_buf)
        .unwrap_or_else(Index::default_path);
    let index = IndexFile::open(&index_path)?;
    if !index.has_printings() {
        bail!(
            "{} names cards by printing, and the index at {} carries no printings to say \
             which card that is.\nRebuild it with: gauntlet sync",
            path.display(),
            index_path.display()
        );
    }
    Ok(deck.to_archidekt(|p| index.printing(&p.set, &p.num).ok().flatten())?)
}

/// Names every card, looking a printing up in the index.
///
/// A deck may name a card by printing and nothing else, so an index without
/// printings cannot say which card it is. That is refused by name rather than
/// reported as an unknown card: the remedy is a sync, not a spelling check.
fn resolve<'a>(
    cards: impl Iterator<Item = &'a deck::Card>,
    index: &IndexFile,
    path: &Path,
) -> Result<Vec<Line>> {
    let mut lines = Vec::new();
    let mut unknown = Vec::new();
    for c in cards {
        let name = match &c.card {
            CardRef::Name(name) => name.clone(),
            CardRef::Printing(p) => {
                if !index.has_printings() {
                    bail!(
                        "this deck names {p} by printing, and the index at {} carries no \
                         printings to say which card that is.\nRebuild it with: gauntlet sync",
                        path.display()
                    );
                }
                match index.printing(&p.set, &p.num)? {
                    Some(name) => name,
                    None => {
                        unknown.push(p.to_string());
                        continue;
                    }
                }
            }
        };
        lines.push(Line {
            name,
            qty: c.qty.get(),
            categories: c.categories.clone(),
            commander: c.is_commander(),
        });
    }
    if !unknown.is_empty() {
        bail!(
            "unknown printing(s), so no number here can be computed:\n  {}\n  No such set and \
             collector number in this card index. Check them; if the printing is newer than \
             the index,\n  rebuild it with `gauntlet sync`.",
            unknown.join(", ")
        );
    }
    Ok(lines)
}

fn unknown_cards(unknown: &[&Line], not_cards: &[&Line]) -> String {
    let (tokens, cards): (Vec<&Line>, Vec<&Line>) = unknown
        .iter()
        .partition(|e| e.categories.iter().any(|c| looks_like_tokens(c)));
    let names = |list: &[&Line]| {
        list.iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = String::from("unknown card(s), so no number here can be computed:");
    if !not_cards.is_empty() {
        out.push_str(&format!(
            "\n  {}\n  Tokens, not cards: the index holds only a token's blank helper record \
             under {}.\n  Mark the category `{{noDeck}}`, as Archidekt's `Tokens & Extras` \
             export does, or delete the line{}.",
            names(not_cards),
            if not_cards.len() == 1 {
                "that name"
            } else {
                "those names"
            },
            if not_cards.len() == 1 { "" } else { "s" }
        ));
    }
    if !cards.is_empty() {
        out.push_str(&format!(
            "\n  {}\n  Not in this card index. Check the spelling; if it is a real card newer \
             than the index,\n  rebuild it with `gauntlet sync`.",
            names(&cards)
        ));
    }
    if !tokens.is_empty() {
        out.push_str(&format!(
            "\n  {}\n  Filed under a token category, and tokens are not cards: the index holds \
             none, and no\n  `sync` will add them. Mark the category `{{noDeck}}`, as \
             Archidekt's `Tokens & Extras`\n  export does, or delete the line{}.",
            names(&tokens),
            if tokens.len() == 1 { "" } else { "s" }
        ));
    }
    out
}

/// Whether a type line names this card type, read by word.
pub(crate) fn names(type_line: &str, kind: &str) -> bool {
    type_line
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(kind))
}

/// The face you play from your hand: the front, or the whole card where the
/// index carries no faces.
pub(crate) fn front(card: &Card) -> &str {
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

/// Whether a card is a creature, read off the face you cast: what an
/// activation on it would have to wait out summoning sickness for (ADR-0019).
pub fn is_creature(card: &Card) -> bool {
    names(front(card), "creature")
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

/// Whether a reanimation may put this card onto the battlefield out of the
/// graveyard (#140): a land, as the land drop reads one, or a permanent card
/// with no land on a face it is not played as.
///
/// A query matches any face, so `t:land` takes in Search for Azcanta by its
/// back, and in the graveyard it is an enchantment card (CR 712.8a). Nothing
/// here can tell which face a query matched, so such a card never comes back:
/// a floor, and it keeps Lumra to the land cards it says. An instant or a
/// sorcery never comes back either, because nothing returns one to the
/// battlefield.
pub fn returnable(card: &Card) -> bool {
    is_land(card) || (is_permanent(card) && !names(&card.type_line, "land"))
}
