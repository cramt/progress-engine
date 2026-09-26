//! Which zone a question is asking about.
//!
//! "Did I find the card" is not a well-formed question. "Is the card in this
//! zone by this turn" is. Every number this tool printed before zones existed
//! silently meant *in hand*, and for a graveyard deck that is the wrong
//! question asked confidently.

use thiserror::Error;

/// A gameplay zone a criterion may count cards in.
///
/// Deliberately not one variant per Magic zone. A variant here is a promise
/// that the engine can answer questions about that zone, so a zone it cannot
/// model has no variant to hide in and is refused by name at the file
/// boundary — the same discipline as an unknown key or an unsupported query
/// term. Exile and the stack have no route into them at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// The default, and what every count in this tool has always meant.
    ///
    /// Today "in hand" and "drawn by turn N" are the same set, because nothing
    /// is cast, discarded or played and so no card ever leaves. That equality
    /// stops holding the day anything routes a card elsewhere, and this is the
    /// name that will still be right when it does.
    Hand,
    /// Askable, and reachable exactly when some effect in this run routes a
    /// card here or the declared line names an instant or a sorcery, which
    /// resolves into its owner's graveyard.
    ///
    /// Selection routing — issues
    /// [#17](https://github.com/cramt/progress-engine/issues/17) and
    /// [#43](https://github.com/cramt/progress-engine/issues/43) — made this a
    /// real destination, and made reachability a property of the run rather
    /// than of the zone. A file whose effects route nothing here still reads
    /// zero by construction, which is why [`Reachable`] is carried from the
    /// run to the report and why a run that asks about an unrouted graveyard
    /// says so out loud.
    ///
    /// **Unordered.** Dredge cares which card is on top of the yard and this
    /// cannot say; "Loam in the graveyard by turn 5" does not care, and that is
    /// the north star. Stated here rather than left to be discovered.
    Graveyard,
    /// What has not been drawn yet.
    ///
    /// Free, and exact: it is the deck's count of matching cards minus the ones
    /// this path has seen. No state to carry, because the engine already knows
    /// both halves.
    Library,
    /// Lands you have played.
    ///
    /// Answerable, and only for a query that picks out lands — which is what
    /// makes it answerable. A land arrives on a land drop, which is free and
    /// capped at one a turn, so *how many are in play* is a function of the
    /// path the enumeration already walks. Every other permanent has to be
    /// cast, and *which* spells you cast is the budget half of
    /// [#10](https://github.com/cramt/progress-engine/issues/10) — so a
    /// battlefield question about anything but a land is still refused by name,
    /// at the boundary where the card data is, rather than answered with
    /// "drawn" under another name.
    ///
    /// **Use-it-or-lose-it**, which is the whole reason this differs from the
    /// hand. Drawing five lands by turn 3 puts three of them in play, not five,
    /// because the other two drops never happened — HANDS.md hand 4.
    Battlefield,
}

/// What a clause is counting: cards sitting in a zone, or cards you paid for.
///
/// A casting is deliberately **not** a [`Zone`] variant. Where a spell ends up
/// after it resolves is a fact about the card — a permanent stays on the
/// battlefield, an instant or a sorcery goes to the graveyard, and the zone
/// counts read it that way — whereas *you cast it* is a fact about the turn,
/// and it is the fact the budget knows. Either can then be undone by something
/// this engine does not model: an opponent's removal, a flashback. Spelling it as a zone
/// would be this tool answering a question it cannot: a Lantern counted on the
/// battlefield would still be there after somebody blew it up.
///
/// So `cast` is its own clause key, and every count in the language goes
/// through this: one place in the engine that answers *how many of these do I
/// have, and in what sense*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Counted {
    /// Cards in a zone at the end of a turn.
    In(Zone),
    /// Cards you have cast by the end of a turn, paid for out of the pool.
    Cast,
}

impl Counted {
    /// The zone this counts in, where it counts one at all.
    pub fn zone(self) -> Option<Zone> {
        match self {
            Counted::In(zone) => Some(zone),
            Counted::Cast => None,
        }
    }
}

impl Zone {
    /// Every zone a criteria file may name, for the message that lists them.
    pub const ACCEPTED: &'static str = "hand, graveyard, library, battlefield";

    /// The zone a clause means when it does not say.
    ///
    /// Silence means the hand, which is what every criteria file written before
    /// zones existed already meant. A file that names no zone gets the numbers
    /// it always got.
    pub const DEFAULT: Zone = Zone::Hand;

    pub fn as_str(self) -> &'static str {
        match self {
            Zone::Hand => "hand",
            Zone::Graveyard => "graveyard",
            Zone::Library => "library",
            Zone::Battlefield => "battlefield",
        }
    }

    /// Read a zone from what a criteria file wrote.
    pub fn parse(name: &str) -> Result<Zone, ZoneError> {
        match name {
            "hand" => Ok(Zone::Hand),
            "graveyard" => Ok(Zone::Graveyard),
            "library" => Ok(Zone::Library),
            "battlefield" => Ok(Zone::Battlefield),
            _ => Err(ZoneError::Unknown {
                name: name.to_string(),
            }),
        }
    }
}

/// Which zones *this run* can actually put a card into.
///
/// A zone nothing routes into answers every question about it with 0.00%, and
/// a zero that means *not modelled* is indistinguishable in a percentage from a
/// zero that means *never happened*. That is this project's defining failure
/// mode, so the answer is still computed — honestly, as zero — and the run
/// reports which zones it was computed in.
///
/// A struct carrying the one contingent zone rather than a method on [`Zone`],
/// because reachability stopped being a property of the zone the day an effect
/// could route a card. The hand and the library are reachable in every run
/// there has ever been; the graveyard is reachable in the runs whose effects
/// send something there, and nothing but the run knows which those are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reachable {
    /// Whether some effect in this run routes a card to the graveyard, or the
    /// declared line casts a spell that resolves into it.
    pub graveyard: bool,
    /// Whether this deck holds a land at all.
    ///
    /// The same confident zero by a different route: a battlefield count in a
    /// landless library is zero because nothing can ever be played, not because
    /// the deck was unlucky.
    pub battlefield: bool,
}

impl Reachable {
    pub fn includes(self, zone: Zone) -> bool {
        match zone {
            Zone::Hand | Zone::Library => true,
            Zone::Graveyard => self.graveyard,
            Zone::Battlefield => self.battlefield,
        }
    }
}

impl std::fmt::Display for Zone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A zone name this engine will not answer questions about.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ZoneError {
    #[error(
        "`zone = {name:?}` is not a zone this tool knows. Accepted: {}.",
        Zone::ACCEPTED
    )]
    Unknown { name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Zone; 4] = [
        Zone::Hand,
        Zone::Graveyard,
        Zone::Library,
        Zone::Battlefield,
    ];

    #[test]
    fn every_zone_reads_back_as_itself() {
        for zone in ALL {
            assert_eq!(Zone::parse(zone.as_str()), Ok(zone));
            assert_eq!(Counted::In(zone).zone(), Some(zone));
        }
        assert_eq!(Counted::Cast.zone(), None);
        assert!(Zone::parse("exile").is_err());
    }

    /// Which zones a run can put a card into decides which zeros are reported
    /// as *not modelled* rather than as *never happened*.
    #[test]
    fn only_the_contingent_zones_depend_on_the_run() {
        let nothing = Reachable {
            graveyard: false,
            battlefield: false,
        };
        let everything = Reachable {
            graveyard: true,
            battlefield: true,
        };
        for zone in [Zone::Hand, Zone::Library] {
            assert!(nothing.includes(zone), "{zone}");
        }
        for zone in [Zone::Graveyard, Zone::Battlefield] {
            assert!(!nothing.includes(zone), "{zone}");
        }
        assert!(ALL.into_iter().all(|zone| everything.includes(zone)));
    }
}
