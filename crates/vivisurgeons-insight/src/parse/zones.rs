//! Zones, and where "put" and "return" send things.

use super::*;

pub(super) fn zone(p: &mut P) -> Option<Zone> {
    alt!(p;
        |p| p.words("a graveyard").map(|_| Zone::AGraveyard),
        |p| p.words("all graveyards").map(|_| Zone::Graveyard(Player::Each)),
        |p| p.word("anywhere").map(|_| Zone::Anywhere),
        |p| p.words("exile").map(|_| Zone::Exile),
        |p| p.words("the battlefield").map(|_| Zone::Battlefield),
        |p| p.words("the command zone").map(|_| Zone::CommandZone),
        |p| p.words("the stack").map(|_| Zone::Stack),
        |p| { p.words("the top of")?; let w = whose(p)?; p.word("library")?; Some(Zone::TopOfLibrary(w)) },
        |p| { p.words("the bottom of")?; let w = whose(p)?; p.word("library")?; Some(Zone::BottomOfLibrary(w)) },
        |p| {
            let w = whose(p)?;
            alt!(p;
                |p| p.word("graveyard").map(|_| Zone::Graveyard(w.clone())),
                |p| p.word("graveyards").map(|_| Zone::Graveyard(w.clone())),
                |p| p.word("hand").map(|_| Zone::Hand(w.clone())),
                |p| p.word("hands").map(|_| Zone::Hand(w.clone())),
                |p| p.word("library").map(|_| Zone::Library(w.clone())),
            )
        },
    )
}

pub(super) fn owners_hand(p: &mut P) -> Option<Zone> {
    alt!(p;
        |p| p.words("its owner 's hand").map(|_| Zone::OwnersHand),
        |p| p.words("their owner 's hand").map(|_| Zone::OwnersHand),
        |p| p.words("their owners ' hands").map(|_| Zone::OwnersHand),
        |p| p.words("their owners 's hands").map(|_| Zone::OwnersHand),
        |p| p.words("their owners hands").map(|_| Zone::OwnersHand),
        |p| p.words("your hand").map(|_| Zone::Hand(Player::You)),
    )
}

pub(super) struct Dest {
    zone: Zone,
    tapped: bool,
    transformed: bool,
    under: Option<Player>,
}

impl Dest {
    pub(super) fn to(zone: Zone) -> Self {
        Dest {
            zone,
            tapped: false,
            transformed: false,
            under: None,
        }
    }

    pub(super) fn moving(self, what: Objects, from: Option<Zone>, order: Option<Order>) -> Effect {
        Effect::Move {
            what,
            from,
            to: self.zone,
            tapped: self.tapped,
            transformed: self.transformed,
            under: self.under,
            order,
        }
    }
}

/// Where "put"/"return" sends something.
pub(super) fn destination(p: &mut P) -> Option<Dest> {
    alt!(p;
        |p| {
            p.words("the battlefield")?;
            let tapped = p.attempt(|p| p.word("tapped")).is_some();
            let transformed = p.attempt(|p| p.word("transformed")).is_some();
            let under = p.attempt(|p| alt!(p;
                |p| p.words("under your control").map(|_| Player::You),
                |p| p.one_of(&["under its owner 's control", "under their owners ' control", "under their owner 's control"]).map(|_| Player::OwnerOf(Box::new(antecedent()))),
            ));
            let tapped = tapped || p.attempt(|p| p.word("tapped")).is_some();
            Some(Dest { zone: Zone::Battlefield, tapped, transformed, under })
        },
        |p| owners_hand(p).map(Dest::to),
        |p| p.words("its owner 's graveyard").map(|_| Dest::to(Zone::OwnersGraveyard)),
        |p| p.words("their owners ' graveyards").map(|_| Dest::to(Zone::OwnersGraveyard)),
        |p| p.words("the top of its owner 's library").map(|_| Dest::to(Zone::OwnersLibraryTop)),
        |p| p.words("the bottom of its owner 's library").map(|_| Dest::to(Zone::OwnersLibraryBottom)),
        |p| p.words("its owner 's library").map(|_| Dest::to(Zone::OwnersLibraryTop)),
        // "put that card on top": of the library it was searched or looked
        // at in, which is always its owner's.
        |p| p.word("top").map(|_| Dest::to(Zone::OwnersLibraryTop)),
        |p| zone(p).map(Dest::to),
    )
}

pub(super) fn order(p: &mut P) -> Option<Order> {
    alt!(p;
        |p| p.words("in any order").map(|_| Order::Any),
        |p| p.words("in a random order").map(|_| Order::Random),
    )
}
