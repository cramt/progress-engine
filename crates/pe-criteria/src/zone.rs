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
/// term. `battlefield` is the one people will reach for and the one that needs
/// castability, which is not modelled; exile and the stack have no route into
/// them at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Zone {
    /// The default, and what every count in this tool has always meant.
    ///
    /// Today "in hand" and "drawn by turn N" are the same set, because nothing
    /// is cast, discarded or played and so no card ever leaves. That equality
    /// stops holding the day anything routes a card elsewhere, and this is the
    /// name that will still be right when it does.
    Hand,
    /// Askable, and correctly always empty.
    ///
    /// Nothing routes a card here yet — that is selection routing, issues
    /// [#17](https://github.com/cramt/progress-engine/issues/17) and
    /// [#43](https://github.com/cramt/progress-engine/issues/43). Until then
    /// every count in it is zero by construction, which is why
    /// [`Zone::is_reachable`] exists and why a run that asks about the
    /// graveyard says so out loud.
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
}

impl Zone {
    /// Every zone a criteria file may name, for the message that lists them.
    pub const ACCEPTED: &'static str = "hand, graveyard, library";

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
        }
    }

    /// Whether anything in this engine can put a card in this zone.
    ///
    /// An unreachable zone answers every question about it with 0.00%, and a
    /// zero that means *not modelled* is indistinguishable in a percentage from
    /// a zero that means *never happened*. That is this project's defining
    /// failure mode, so the answer is still computed — honestly, as zero — and
    /// the run reports which zones it was computed in.
    pub fn is_reachable(self) -> bool {
        match self {
            Zone::Hand | Zone::Library => true,
            Zone::Graveyard => false,
        }
    }

    /// Read a zone from what a criteria file wrote.
    pub fn parse(name: &str) -> Result<Zone, ZoneError> {
        match name {
            "hand" => Ok(Zone::Hand),
            "graveyard" => Ok(Zone::Graveyard),
            "library" => Ok(Zone::Library),
            "battlefield" => Err(ZoneError::Battlefield),
            _ => Err(ZoneError::Unknown {
                name: name.to_string(),
            }),
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
    /// Refused by name rather than approximated.
    ///
    /// The tempting approximation is "held it, so it is in play", and it is
    /// wrong in the direction that flatters the deck: an opening hand with one
    /// Island and a three-drop has the three-drop in hand on turn 0 and on the
    /// battlefield on no turn at all. Answering anyway would report a number
    /// that looks exactly like a measurement.
    #[error(
        "`zone = \"battlefield\"` is not modelled. Knowing a card is on the battlefield means \
         knowing you could cast it, and castability needs the mana model \
         (https://github.com/cramt/progress-engine/issues/10).\n\
         Approximating it would report \"drawn\" under another name, so it is refused instead. \
         Ask about `hand`, and know that is what you asked."
    )]
    Battlefield,
    #[error(
        "`zone = {name:?}` is not a zone this tool knows. Accepted: {}.",
        Zone::ACCEPTED
    )]
    Unknown { name: String },
}
