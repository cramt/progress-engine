//! Where a platform keeps the copy, and when it makes a new one (ADR-0030,
//! ADR-0031): the decisions, with none of the I/O.
//!
//! A platform keeps the copy's text, gzipped, in one of two [`Slot`]s, and a
//! small [`META`] file, written last, names the slot holding a whole copy. A
//! new copy goes in the slot the meta does not name, so a copy cut short, by
//! a closed tab, a killed process or a full disk, is never read and never
//! costs the old one. A Default Cards file the builder refused is remembered
//! by its `updated_at` in [`UNREADABLE`], so it is not downloaded again only
//! to be refused again.
//!
//! A platform drives a [`Store`] through one look at a time, under a lock of
//! its own so two never build at once:
//!
//! 1. Read [`META`] and [`Meta::parse`] it. [`Store::newer`] says whether it
//!    names a copy newer than the one held; if so, open that slot and
//!    [`Store::hold`] it.
//! 2. Whether to ask Scryfall's `/bulk-data` at all is the platform's call.
//!    The browser asks when [`Store::due`] says the copy is a day old;
//!    natively only `gauntlet sync` asks, and nothing refreshes on its own.
//! 3. With the listing's `updated_at` and [`UNREADABLE`]'s text,
//!    [`Store::plan`] says whether the copy held is current, whether the file
//!    is one already refused, or which slot to [`Build`] into.
//! 4. Feed the bulk files to a [`super::Builder`]. Write its text to
//!    [`Build::slot`], then [`Store::built`]'s meta to [`META`], in that order.
//!    If the builder refused the file, write [`Store::unreadable`]'s text to
//!    [`UNREADABLE`] instead.

use facet::Facet;

use super::FORMAT;

/// The file naming the slot that holds a whole copy, written last.
pub const META: &str = "scryfall-copy.meta.json";

/// The `updated_at` of a Default Cards file this build's [`super::Builder`]
/// refused. Asking again would download the same 85 MB to fail the same way,
/// so the copy held keeps answering until Scryfall writes a new file.
pub const UNREADABLE: &str = "scryfall-copy.unreadable.txt";

/// How often an open tab looks whether its copy is [`Store::due`], so a tab
/// left open for days still has the day's prices.
pub const LOOK: UnixMs = UnixMs(60 * 60 * 1000);

/// Scryfall rewrites its bulk files every 12 to 24 hours, and with them the
/// day's prices, so a copy a day old is asked about once.
pub const REFRESH: UnixMs = UnixMs(24 * 60 * 60 * 1000);

/// Milliseconds since the Unix epoch, as `Date.now()` and `SystemTime` give
/// them. Also a span of them, for [`LOOK`] and [`REFRESH`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnixMs(pub u64);

/// One of the two places a copy is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

impl Slot {
    pub fn file(self) -> &'static str {
        match self {
            Slot::A => "scryfall-copy.a.txt.gz",
            Slot::B => "scryfall-copy.b.txt.gz",
        }
    }

    pub fn other(self) -> Slot {
        match self {
            Slot::A => Slot::B,
            Slot::B => Slot::A,
        }
    }

    fn from_file(file: &str) -> Option<Slot> {
        [Slot::A, Slot::B].into_iter().find(|s| s.file() == file)
    }
}

/// A whole copy that storage holds: which slot, the Default Cards file it was
/// made from, and when it was last made or found current.
///
/// Only a meta of this build's [`FORMAT`] parses, so a `Meta` names a copy
/// this build can read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    slot: Slot,
    updated_at: String,
    checked_at: UnixMs,
}

/// Why a meta file names no copy this build can read. Each means the same to
/// a platform, that storage holds no copy, and is told apart for the tests
/// and the console.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum MetaRefused {
    #[error("the meta file is not one: {0}")]
    Unreadable(String),
    #[error("the copy is format {0}, and this build reads {FORMAT}")]
    Format(u32),
    #[error("the meta file names no slot this build keeps, but {0:?}")]
    Slot(String),
}

/// The meta file as written. The field names are what Curator's worker wrote
/// before the policy was Rust's, so a copy kept then is read without
/// downloading it again.
#[derive(Facet)]
#[facet(rename_all = "camelCase")]
struct MetaFile {
    format: u32,
    slot: String,
    updated_at: String,
    checked_at: u64,
}

impl Meta {
    pub fn parse(text: &str) -> Result<Meta, MetaRefused> {
        let file: MetaFile =
            facet_json::from_str(text).map_err(|e| MetaRefused::Unreadable(e.to_string()))?;
        if file.format != FORMAT {
            return Err(MetaRefused::Format(file.format));
        }
        let slot = Slot::from_file(&file.slot).ok_or(MetaRefused::Slot(file.slot))?;
        Ok(Meta {
            slot,
            updated_at: file.updated_at,
            checked_at: UnixMs(file.checked_at),
        })
    }

    pub fn to_text(&self) -> String {
        facet_json::to_string(&MetaFile {
            format: FORMAT,
            slot: self.slot.file().to_string(),
            updated_at: self.updated_at.clone(),
            checked_at: self.checked_at.0,
        })
        .expect("a meta serialises")
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    /// Scryfall's `updated_at` for the Default Cards file the copy was made
    /// from.
    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    /// ISO 8601 times written by one server sort as text.
    fn newer_than(&self, other: Option<&Meta>) -> bool {
        other.is_none_or(|o| self.updated_at > o.updated_at)
    }
}

/// A new copy being made: from which Default Cards file, into which slot.
/// Only [`Store::plan`] and [`Build::over`] make one, so the slot is never
/// the one storage's meta names.
#[derive(Debug, PartialEq, Eq)]
pub struct Build {
    slot: Slot,
    updated_at: String,
}

impl Build {
    /// A copy of the file Scryfall wrote at `updated_at`, built whether or
    /// not storage holds it already: what `gauntlet sync --force` asks for.
    /// It goes in the slot `kept`, storage's meta, does not name.
    pub fn over(kept: Option<&Meta>, updated_at: &str) -> Build {
        Build {
            slot: kept.map_or(Slot::A, |k| k.slot.other()),
            updated_at: updated_at.to_string(),
        }
    }

    pub fn slot(&self) -> Slot {
        self.slot
    }

    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }
}

/// What [`Store::plan`] decided.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Scryfall has written nothing newer than the copy held. `write` is the
    /// meta to keep, saying so, when storage holds that copy too.
    Current {
        write: Option<Meta>,
    },
    /// Scryfall's file is one this build already refused, and the copy held,
    /// if any, keeps answering.
    Refused {
        updated_at: String,
    },
    Build(Build),
}

/// What a platform holds of the copy between looks: the copy it answers from,
/// and a file it could not read. Platforms keep one for as long as they run.
#[derive(Debug, Default)]
pub struct Store {
    holding: Option<Meta>,
    unreadable: Option<String>,
}

impl Store {
    pub fn new() -> Store {
        Store::default()
    }

    /// The copy answering, which need not be storage's: when storage could
    /// not keep a copy this made, it answers from that one in memory.
    pub fn holding(&self) -> Option<&Meta> {
        self.holding.as_ref()
    }

    /// `kept`, storage's meta, if it names a copy newer than the one held,
    /// which another tab made, perhaps while this one waited for the lock.
    /// Storage's older copy is never opened over a newer one held.
    pub fn newer<'a>(&self, kept: Option<&'a Meta>) -> Option<&'a Meta> {
        kept.filter(|k| k.newer_than(self.holding.as_ref()))
    }

    /// The copy `meta` names is open and answers. Natively, where nothing is
    /// opened to decide whether to sync, the copy on disk is the one held.
    pub fn hold(&mut self, meta: Meta) {
        self.holding = Some(meta);
    }

    /// Whether the copy held is old enough to ask `/bulk-data` about: a day
    /// since it was made or found current, or no copy at all.
    pub fn due(&self, now: UnixMs) -> bool {
        self.holding
            .as_ref()
            .is_none_or(|h| now.0.saturating_sub(h.checked_at.0) >= REFRESH.0)
    }

    /// Given Scryfall's listing, what to do about it. `kept` is storage's meta,
    /// read in this look; `listing` the `updated_at` `/bulk-data` gives for
    /// Default Cards; `unreadable` the text of [`UNREADABLE`], if any.
    pub fn plan(
        &mut self,
        kept: Option<&Meta>,
        listing: &str,
        unreadable: Option<&str>,
        now: UnixMs,
    ) -> Plan {
        if let Some(held) = self.holding.as_mut().filter(|h| h.updated_at == listing) {
            held.checked_at = now;
            let write = kept.filter(|k| k.updated_at == listing).map(|k| Meta {
                checked_at: now,
                ..k.clone()
            });
            return Plan::Current { write };
        }
        // Kept in memory too, for when storage could not keep the file.
        if self.unreadable.is_none() {
            self.unreadable = unreadable.map(|u| u.trim().to_string());
        }
        if self.unreadable.as_deref() == Some(listing) {
            return Plan::Refused {
                updated_at: listing.to_string(),
            };
        }
        Plan::Build(Build::over(kept, listing))
    }

    /// `build` is done and its copy answers. The meta to write once its text
    /// is in [`Build::slot`], and not before.
    pub fn built(&mut self, build: Build, now: UnixMs) -> Meta {
        let made = Meta {
            slot: build.slot,
            updated_at: build.updated_at,
            checked_at: now,
        };
        self.holding = Some(made.clone());
        made
    }

    /// The builder refused `build`'s file. The text to write to
    /// [`UNREADABLE`], and remembered here in case storage cannot keep it.
    pub fn unreadable(&mut self, build: Build) -> String {
        self.unreadable = Some(build.updated_at.clone());
        build.updated_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: &str = "2026-10-05T09:05:44.334+00:00";
    const TUE: &str = "2026-10-06T09:05:44.334+00:00";
    const WED: &str = "2026-10-07T09:05:44.334+00:00";
    const NOW: UnixMs = UnixMs(1_760_000_000_000);
    const HOUR: u64 = 60 * 60 * 1000;

    fn later(hours: u64) -> UnixMs {
        UnixMs(NOW.0 + hours * HOUR)
    }

    /// What storage holds after a copy of `updated_at` was made into `slot`
    /// at [`NOW`], as a platform reads it back.
    fn kept(slot: Slot, updated_at: &str) -> Meta {
        let mut store = Store::new();
        let meta = store.built(
            Build {
                slot,
                updated_at: updated_at.into(),
            },
            NOW,
        );
        Meta::parse(&meta.to_text()).unwrap()
    }

    /// A store that has opened `meta`, as a page load does.
    fn holding(meta: &Meta) -> Store {
        let mut store = Store::new();
        let open = store.newer(Some(meta)).expect("nothing held yet");
        store.hold(open.clone());
        store
    }

    #[test]
    fn a_meta_the_worker_wrote_before_the_policy_was_rust_still_parses() {
        let text = format!(
            r#"{{"format":{FORMAT},"slot":"scryfall-copy.b.txt.gz","updatedAt":"{MON}","checkedAt":1760000000000}}"#
        );
        let meta = Meta::parse(&text).unwrap();
        assert_eq!(meta.slot(), Slot::B);
        assert_eq!(meta.updated_at(), MON);
        assert_eq!(meta.checked_at, NOW);
        assert_eq!(Meta::parse(&meta.to_text()).unwrap(), meta);
    }

    #[test]
    fn a_meta_of_another_format_or_slot_names_no_copy() {
        let other = format!(
            r#"{{"format":{},"slot":"scryfall-copy.a.txt.gz","updatedAt":"{MON}","checkedAt":0}}"#,
            FORMAT - 1
        );
        assert_eq!(Meta::parse(&other), Err(MetaRefused::Format(FORMAT - 1)));
        let slot = format!(
            r#"{{"format":{FORMAT},"slot":"../../etc/passwd","updatedAt":"{MON}","checkedAt":0}}"#
        );
        assert_eq!(
            Meta::parse(&slot),
            Err(MetaRefused::Slot("../../etc/passwd".into()))
        );
        assert!(matches!(
            Meta::parse(r#"{"format":4,"slot""#),
            Err(MetaRefused::Unreadable(_))
        ));
        assert!(matches!(Meta::parse(""), Err(MetaRefused::Unreadable(_))));
    }

    #[test]
    fn a_new_copy_goes_in_the_slot_storage_does_not_name() {
        let mut first = Store::new();
        let Plan::Build(build) = first.plan(None, MON, None, NOW) else {
            panic!("nothing kept, so build");
        };
        assert_eq!(build.slot(), Slot::A);
        for slot in [Slot::A, Slot::B] {
            let k = kept(slot, MON);
            let mut store = holding(&k);
            let Plan::Build(build) = store.plan(Some(&k), TUE, None, later(25)) else {
                panic!("a newer file is built");
            };
            assert_eq!(build.slot(), slot.other());
        }
    }

    /// Another tab moved storage on to B while this one held A: the next
    /// copy goes in A, never over B, whatever this tab answers from.
    #[test]
    fn the_slot_is_storages_other_not_the_holders() {
        let a = kept(Slot::A, MON);
        let b = kept(Slot::B, TUE);
        let mut store = holding(&a);
        let Plan::Build(build) = store.plan(Some(&b), WED, None, later(25)) else {
            panic!("a newer file is built");
        };
        assert_eq!(build.slot(), Slot::A);
    }

    /// The meta naming the new slot exists only from [`Store::built`], after
    /// the build: until then, storage's meta, and anything read from it,
    /// still names the old copy.
    #[test]
    fn the_meta_names_the_new_copy_only_once_it_is_built() {
        let k = kept(Slot::A, MON);
        let mut store = holding(&k);
        let Plan::Build(build) = store.plan(Some(&k), TUE, None, later(25)) else {
            panic!("a newer file is built");
        };
        assert_eq!(store.holding(), Some(&k));
        let made = store.built(build, later(26));
        assert_eq!(made.slot(), Slot::B);
        assert_eq!(made.updated_at(), TUE);
        assert_eq!(store.holding(), Some(&made));
        assert_eq!(Meta::parse(&made.to_text()).unwrap(), made);
    }

    /// The tab closes mid-build: the meta was never written, so the next load
    /// reads storage's meta as it was, opens the old copy, and builds into
    /// the same other slot again rather than over the old one.
    #[test]
    fn a_copy_cut_short_is_never_read_and_never_costs_the_old_one() {
        let k = kept(Slot::A, MON);
        let text = k.to_text();
        let mut tab = holding(&k);
        let Plan::Build(cut) = tab.plan(Some(&k), TUE, None, later(25)) else {
            panic!("a newer file is built");
        };
        drop((tab, cut));

        let reread = Meta::parse(&text).unwrap();
        let mut reopened = Store::new();
        let open = reopened.newer(Some(&reread)).unwrap();
        assert_eq!((open.slot(), open.updated_at()), (Slot::A, MON));
        reopened.hold(open.clone());
        assert!(reopened.due(later(25)));
        let Plan::Build(again) = reopened.plan(Some(&reread), TUE, None, later(25)) else {
            panic!("the cut-short build is made again");
        };
        assert_eq!(again.slot(), Slot::B);
    }

    #[test]
    fn a_file_the_builder_refused_is_not_downloaded_again() {
        let k = kept(Slot::A, MON);
        let mut tab = holding(&k);
        let Plan::Build(build) = tab.plan(Some(&k), TUE, None, later(25)) else {
            panic!("a newer file is built");
        };
        let written = tab.unreadable(build);
        // The same tab an hour on, even when storage could not keep the file.
        assert_eq!(
            tab.plan(Some(&k), TUE, None, later(26)),
            Plan::Refused {
                updated_at: TUE.into()
            }
        );
        // A page load, reading the file back.
        let mut reload = holding(&k);
        assert_eq!(
            reload.plan(Some(&k), TUE, Some(&format!("{written}\n")), later(27)),
            Plan::Refused {
                updated_at: TUE.into()
            }
        );
        // With no copy at all, too.
        assert!(matches!(
            Store::new().plan(None, TUE, Some(&written), later(27)),
            Plan::Refused { .. }
        ));
        // Scryfall writes a new file, and that one is tried.
        assert!(matches!(
            reload.plan(Some(&k), WED, Some(&written), later(48)),
            Plan::Build(_)
        ));
    }

    /// The old copy stays the one held, and so answering, until a build is
    /// done; and a refused or abandoned build leaves it held.
    #[test]
    fn the_old_copy_answers_while_a_new_one_builds() {
        let k = kept(Slot::A, MON);
        let mut store = holding(&k);
        let Plan::Build(build) = store.plan(Some(&k), TUE, None, later(25)) else {
            panic!("a newer file is built");
        };
        assert_eq!(store.holding(), Some(&k));
        store.unreadable(build);
        assert_eq!(store.holding(), Some(&k));
    }

    #[test]
    fn storages_copy_is_opened_only_when_newer_than_the_one_held() {
        let mon = kept(Slot::A, MON);
        let tue = kept(Slot::B, TUE);
        let store = holding(&tue);
        // This tab made Tuesday's copy and storage could not keep it.
        assert_eq!(store.newer(Some(&mon)), None);
        assert_eq!(store.newer(Some(&tue)), None);
        assert_eq!(store.newer(None), None);
        // Another tab made a newer copy.
        let store = holding(&mon);
        assert_eq!(store.newer(Some(&tue)), Some(&tue));
    }

    #[test]
    fn a_copy_is_asked_about_once_it_is_a_day_old() {
        assert!(Store::new().due(NOW));
        let store = holding(&kept(Slot::A, MON));
        assert!(!store.due(NOW));
        assert!(!store.due(later(23)));
        assert!(store.due(later(24)));
        // A clock that went back is not a day on.
        assert!(!store.due(UnixMs(NOW.0 - HOUR)));
        assert!(LOOK < REFRESH);
    }

    /// Scryfall has nothing newer: the copy held is current for another day,
    /// and the meta says so only when it names that same copy.
    #[test]
    fn a_current_copy_is_checked_again_a_day_on() {
        let k = kept(Slot::A, MON);
        let mut store = holding(&k);
        let Plan::Current { write: Some(write) } = store.plan(Some(&k), MON, None, later(25))
        else {
            panic!("storage holds the copy held");
        };
        assert_eq!((write.slot(), write.updated_at()), (Slot::A, MON));
        assert_eq!(write.checked_at, later(25));
        assert!(!store.due(later(48)));
        assert!(store.due(later(49)));

        // Storage could not keep the copy this tab made, and has an older one.
        let older = kept(Slot::B, "2026-10-04T09:05:44.334+00:00");
        assert_eq!(
            store.plan(Some(&older), MON, None, later(49)),
            Plan::Current { write: None }
        );
    }

    #[test]
    fn a_current_copy_is_current_even_if_its_file_was_once_refused() {
        let k = kept(Slot::A, MON);
        let mut store = holding(&k);
        assert!(matches!(
            store.plan(Some(&k), MON, Some(MON), later(25)),
            Plan::Current { .. }
        ));
    }

    #[test]
    fn a_forced_build_goes_beside_the_copy_kept() {
        let k = kept(Slot::B, MON);
        let build = Build::over(Some(&k), MON);
        assert_eq!((build.slot(), build.updated_at()), (Slot::A, MON));
        assert_eq!(Build::over(None, MON).slot(), Slot::A);
    }
}
