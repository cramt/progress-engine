//! The replay's ratchet: each milestone game's end, as `chancellor-logs
//! replay --json` writes it, and what a change may not do to one (README,
//! "The milestone"). `chancellor-logs gate` reads two runs and fails on
//! any breach, game by game, so a batch can't pass on totals that a
//! divergence traded for a finish elsewhere would leave unmoved.

use std::collections::BTreeMap;
use std::fmt;

use facet::Facet;

use crate::log;
use crate::replay::{End, Replay, Stop};

/// One run of the replay: each milestone's games.
#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub milestones: Vec<MilestoneGames>,
}

#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct MilestoneGames {
    pub name: String,
    pub games: Vec<GameEnd>,
}

/// How one game's replay ended.
#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct GameEnd {
    pub game: u64,
    /// The game's lines.
    pub lines: usize,
    pub end: Ended,
    /// The cards no one named the replay presumed had no ability in a
    /// graveyard (`Replay::presumed`).
    pub presumed: usize,
}

/// `End`, its stop by kind and as the report says it.
#[derive(Facet, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ended {
    Finished,
    Stopped {
        at: usize,
        stop: StopKind,
        said: String,
    },
    /// `End::Masked`: a refusal at `at`, every way past it diverged at
    /// `diverged_at`.
    Masked {
        at: usize,
        stop: StopKind,
        said: String,
        diverged_at: usize,
        diverged: String,
    },
}

/// `Stop`'s variant.
#[derive(Facet, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StopKind {
    Setup,
    Refused,
    Illegal,
    Diverged,
    GaveUp,
    Unfollowed,
    UnknownCard,
}

impl StopKind {
    pub fn of(stop: &Stop) -> Self {
        match stop {
            Stop::Setup(_) => StopKind::Setup,
            Stop::Refused { .. } => StopKind::Refused,
            Stop::Illegal { .. } => StopKind::Illegal,
            Stop::Diverged(_) => StopKind::Diverged,
            Stop::GaveUp(_) => StopKind::GaveUp,
            Stop::Unfollowed { .. } => StopKind::Unfollowed,
            Stop::UnknownCard(_) => StopKind::UnknownCard,
        }
    }
}

impl Ended {
    pub fn of(end: &End) -> Self {
        match end {
            End::Finished => Ended::Finished,
            End::Stopped { entry, stop } => Ended::Stopped {
                at: *entry,
                stop: StopKind::of(stop),
                said: stop.to_string(),
            },
            End::Masked {
                entry,
                stop,
                diverged,
            } => Ended::Masked {
                at: *entry,
                stop: StopKind::of(stop),
                said: stop.to_string(),
                diverged_at: diverged.entry,
                diverged: format!("{:?}", diverged.divergence),
            },
        }
    }

    pub fn stop(&self) -> Option<StopKind> {
        match self {
            Ended::Finished => None,
            Ended::Stopped { stop, .. } | Ended::Masked { stop, .. } => Some(*stop),
        }
    }
}

impl fmt::Display for Ended {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ended::Finished => write!(f, "to the end"),
            Ended::Stopped { at, said, .. } => write!(f, "line {at}: {said}"),
            Ended::Masked {
                at,
                said,
                diverged_at,
                diverged,
                ..
            } => write!(
                f,
                "line {at}: {said}, every way past it diverging at line {diverged_at}: {diverged}"
            ),
        }
    }
}

impl GameEnd {
    pub fn of(g: &log::Game, replayed: &Replay) -> Self {
        GameEnd {
            game: g.id,
            lines: g.entries.len(),
            end: Ended::of(&replayed.end),
            presumed: replayed.presumed,
        }
    }

    /// The lines before its stop, all of them to the end.
    pub fn followed(&self) -> usize {
        match &self.end {
            Ended::Finished => self.lines,
            Ended::Stopped { at, .. } | Ended::Masked { at, .. } => *at,
        }
    }
}

/// What a batch names it did on purpose, game by game, in the allow file
/// `gate --allow` reads: `<kind> <game> <why>` a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Allow {
    /// A silent guess turned into a refusal: the game may follow fewer
    /// lines or no longer finish, if it now stops at a refusal.
    Refusal,
    /// A new gave-up the batch showed is the search, not the engine, by
    /// forcing the finds by hand where a rerun with a bigger budget
    /// wasn't enough.
    GaveUp,
    /// A game that newly relies on a card no one named in a graveyard
    /// having no ability there.
    Presumed,
}

impl Allow {
    fn word(self) -> &'static str {
        match self {
            Allow::Refusal => "refusal",
            Allow::GaveUp => "gave-up",
            Allow::Presumed => "presumed",
        }
    }
}

/// The allow file: each game's allowances, with why.
#[derive(Clone, Debug, Default)]
pub struct Allowed(BTreeMap<(u64, Allow), String>);

impl Allowed {
    /// `#` comments and blank lines aside, each line is `<kind> <game>
    /// <why>`, kind `refusal`, `gave-up` or `presumed`, and the why said.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut out = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |what: &str| format!("allow line {}: {what}: {line}", n + 1);
            let mut words = line.splitn(3, char::is_whitespace);
            let kind = match words.next() {
                Some("refusal") => Allow::Refusal,
                Some("gave-up") => Allow::GaveUp,
                Some("presumed") => Allow::Presumed,
                _ => return Err(bad("not refusal, gave-up or presumed")),
            };
            let game: u64 = words
                .next()
                .and_then(|g| g.parse().ok())
                .ok_or_else(|| bad("no game id"))?;
            let why = words.next().map(str::trim).unwrap_or("");
            if why.is_empty() {
                return Err(bad("no reason given"));
            }
            out.insert((game, kind), why.to_string());
        }
        Ok(Allowed(out))
    }

    fn get(&self, game: u64, kind: Allow) -> Option<&str> {
        self.0.get(&(game, kind)).map(String::as_str)
    }
}

/// Something a change did to a milestone game that the gate fails on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Breach {
    /// The baseline replayed it and this run doesn't.
    Missing { game: u64 },
    /// It diverges where the baseline didn't, or otherwise.
    Diverges { game: u64, now: Ended },
    /// A divergence behind a refusal the baseline didn't have.
    Masked { game: u64, now: Ended },
    /// It gives up going back where the baseline didn't, or otherwise:
    /// maybe the search, maybe a divergence.
    GivesUp { game: u64, at: usize, now: Ended },
    /// It follows fewer lines than the baseline.
    Fewer {
        game: u64,
        was: Ended,
        followed: (usize, usize),
        now: Ended,
    },
    /// It relies on a card presumed in a graveyard where the baseline
    /// didn't.
    Presumes { game: u64, presumed: usize },
}

impl Breach {
    pub fn game(&self) -> u64 {
        match self {
            Breach::Missing { game }
            | Breach::Diverges { game, .. }
            | Breach::Masked { game, .. }
            | Breach::GivesUp { game, .. }
            | Breach::Fewer { game, .. }
            | Breach::Presumes { game, .. } => *game,
        }
    }

    /// The allowance that lets it pass, if the allow file can.
    fn allowed_by(&self) -> Option<Allow> {
        match self {
            Breach::GivesUp { .. } => Some(Allow::GaveUp),
            Breach::Presumes { .. } => Some(Allow::Presumed),
            // Only a game that now stops at a refusal followed its lost
            // lines by a guess.
            Breach::Fewer { now, .. } if now.stop() == Some(StopKind::Refused) => {
                Some(Allow::Refusal)
            }
            _ => None,
        }
    }
}

impl fmt::Display for Breach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Breach::Missing { game } => write!(f, "game {game}: no longer replayed"),
            Breach::Diverges { game, now } => write!(f, "game {game}: diverges, {now}"),
            Breach::Masked { game, now } => {
                write!(f, "game {game}: a divergence masked by a refusal, {now}")
            }
            Breach::GivesUp { game, now, .. } => write!(f, "game {game}: gives up going back, {now}"),
            Breach::Fewer {
                game,
                was,
                followed: (before, after),
                now,
            } => write!(
                f,
                "game {game}: follows {after} lines, from {before}; was {was}, now {now}"
            ),
            Breach::Presumes { game, presumed } => write!(
                f,
                "game {game}: newly presumes {presumed} cards no one named in a graveyard have no ability there"
            ),
        }
    }
}

/// The gate's verdict: the breaches nothing allows, those the allow file
/// does with its reason, and the allowances no breach used.
#[derive(Debug, Default)]
pub struct Verdict {
    pub breaches: Vec<Breach>,
    pub allowed: Vec<(Breach, String)>,
    pub unused: Vec<String>,
}

/// Every breach of `after` against `baseline`, milestone by milestone,
/// those the allow file names set apart.
pub fn check(baseline: &Run, after: &Run, allowed: &Allowed) -> Verdict {
    let mut verdict = Verdict::default();
    let mut used = Vec::new();
    for ms in &baseline.milestones {
        let now: BTreeMap<u64, &GameEnd> = after
            .milestones
            .iter()
            .filter(|a| a.name == ms.name)
            .flat_map(|a| &a.games)
            .map(|g| (g.game, g))
            .collect();
        for was in &ms.games {
            let Some(now) = now.get(&was.game) else {
                verdict.breaches.push(Breach::Missing { game: was.game });
                continue;
            };
            for breach in breaches(was, now) {
                match breach
                    .allowed_by()
                    .and_then(|kind| allowed.get(was.game, kind).map(|why| (kind, why)))
                {
                    Some((kind, why)) => {
                        used.push((was.game, kind));
                        verdict.allowed.push((breach, why.to_string()));
                    }
                    None => verdict.breaches.push(breach),
                }
            }
        }
    }
    verdict.unused = allowed
        .0
        .keys()
        .filter(|k| !used.contains(k))
        .map(|(game, kind)| format!("{} {game}", kind.word()))
        .collect();
    verdict
}

/// One game's breaches: an end it didn't have before that the engine may
/// be wrong at, fewer lines, a presumption.
fn breaches(was: &GameEnd, now: &GameEnd) -> Vec<Breach> {
    let game = now.game;
    let mut out = Vec::new();
    let changed = was.end != now.end;
    match &now.end {
        Ended::Stopped {
            stop: StopKind::Diverged,
            ..
        } if changed => out.push(Breach::Diverges {
            game,
            now: now.end.clone(),
        }),
        Ended::Stopped {
            stop: StopKind::GaveUp,
            at,
            ..
        } if changed => out.push(Breach::GivesUp {
            game,
            at: *at,
            now: now.end.clone(),
        }),
        Ended::Masked { .. } if changed => out.push(Breach::Masked {
            game,
            now: now.end.clone(),
        }),
        _ => {}
    }
    if now.followed() < was.followed() {
        out.push(Breach::Fewer {
            game,
            was: was.end.clone(),
            followed: (was.followed(), now.followed()),
            now: now.end.clone(),
        });
    }
    if now.presumed > 0 && was.presumed == 0 {
        out.push(Breach::Presumes {
            game,
            presumed: now.presumed,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(game: u64, end: Ended, presumed: usize) -> GameEnd {
        GameEnd {
            game,
            lines: 100,
            end,
            presumed,
        }
    }

    fn stopped(at: usize, stop: StopKind, said: &str) -> Ended {
        Ended::Stopped {
            at,
            stop,
            said: said.into(),
        }
    }

    fn run(games: Vec<GameEnd>) -> Run {
        Run {
            milestones: vec![MilestoneGames {
                name: "replay-500".into(),
                games,
            }],
        }
    }

    fn refused(at: usize) -> Ended {
        stopped(at, StopKind::Refused, "refuses X: y")
    }

    #[test]
    fn a_run_against_itself_passes() {
        let r = run(vec![
            game(1, Ended::Finished, 0),
            game(2, stopped(50, StopKind::GaveUp, "gave up: Unpayable"), 0),
            game(3, refused(10), 2),
        ]);
        let v = check(&r, &r, &Allowed::default());
        assert!(v.breaches.is_empty(), "{:?}", v.breaches);
    }

    /// Totals don't move when one game finishes and another starts to
    /// diverge, and a gave-up swapped for another leaves the count; game by
    /// game, both fail.
    #[test]
    fn a_new_divergence_or_gave_up_fails_whatever_the_totals() {
        let before = run(vec![
            game(1, refused(40), 0),
            game(2, stopped(50, StopKind::GaveUp, "gave up: A"), 0),
            game(3, stopped(60, StopKind::Unfollowed, "doesn't follow"), 0),
        ]);
        let after = run(vec![
            game(1, stopped(70, StopKind::Diverged, "diverged: Unpayable"), 0),
            game(2, stopped(55, StopKind::GaveUp, "gave up: B"), 0),
            game(3, stopped(60, StopKind::GaveUp, "gave up: A"), 0),
        ]);
        let v = check(&before, &after, &Allowed::default());
        let games: Vec<_> = v.breaches.iter().map(|b| (b.game(), b)).collect();
        assert!(
            matches!(games[0], (1, Breach::Diverges { .. })),
            "{games:?}"
        );
        assert!(
            matches!(games[1], (2, Breach::GivesUp { at: 55, .. })),
            "{games:?}"
        );
        assert!(matches!(games[2], (3, Breach::GivesUp { .. })), "{games:?}");
        assert_eq!(games.len(), 3, "{games:?}");
    }

    #[test]
    fn a_masked_divergence_fails_unless_the_baseline_had_it() {
        let masked = |d: &str| Ended::Masked {
            at: 83,
            stop: StopKind::Refused,
            said: "refuses Accumulated Knowledge".into(),
            diverged_at: 85,
            diverged: d.into(),
        };
        let before = run(vec![
            game(1, masked("Discards { log: 1, engine: 4 }"), 1),
            game(2, refused(83), 1),
        ]);
        let after = run(vec![
            game(1, masked("Discards { log: 1, engine: 4 }"), 1),
            game(2, masked("HandSize { log: 3, engine: 5 }"), 1),
        ]);
        let v = check(&before, &after, &Allowed::default());
        assert_eq!(v.breaches.len(), 1, "{:?}", v.breaches);
        assert!(matches!(v.breaches[0], Breach::Masked { game: 2, .. }));
    }

    #[test]
    fn fewer_lines_pass_only_as_a_named_refusal() {
        let before = run(vec![
            game(1, Ended::Finished, 0),
            game(2, refused(60), 0),
            game(3, refused(60), 0),
        ]);
        let after = run(vec![
            game(1, refused(90), 0),
            game(2, refused(40), 0),
            game(3, stopped(40, StopKind::Unfollowed, "doesn't follow"), 0),
        ]);
        let allowed = Allowed::parse(
            "# turned guesses into refusals\nrefusal 1 a hidden card read as nothing\nrefusal 3 x\ngave-up 9 unused\n",
        )
        .unwrap();
        let v = check(&before, &after, &allowed);
        let failing: Vec<u64> = v.breaches.iter().map(Breach::game).collect();
        // 3 now stops at a line not followed, not a refusal: its allow
        // doesn't reach it.
        assert_eq!(failing, vec![2, 3], "{:?}", v.breaches);
        assert_eq!(v.allowed.len(), 1);
        assert_eq!(v.unused, vec!["refusal 3", "gave-up 9"]);
    }

    #[test]
    fn a_game_newly_presuming_or_no_longer_replayed_fails() {
        let before = run(vec![game(1, refused(60), 0), game(2, refused(60), 3)]);
        let after = run(vec![game(1, refused(60), 2)]);
        let v = check(&before, &after, &Allowed::default());
        assert_eq!(
            v.breaches,
            vec![
                Breach::Presumes {
                    game: 1,
                    presumed: 2
                },
                Breach::Missing { game: 2 }
            ]
        );
        let v = check(
            &before,
            &after,
            &Allowed::parse("presumed 1 strategic planning now reached").unwrap(),
        );
        assert_eq!(v.breaches, vec![Breach::Missing { game: 2 }]);
    }

    #[test]
    fn an_allow_line_needs_a_kind_a_game_and_a_reason() {
        assert!(Allowed::parse("refusal 12").is_err());
        assert!(Allowed::parse("fewer 12 because").is_err());
        assert!(Allowed::parse("gave-up x because").is_err());
        assert!(Allowed::parse("gave-up 12 forced the finds: line 40 past").is_ok());
    }

    #[test]
    fn a_run_reads_back_as_written() {
        let r = run(vec![
            game(1, Ended::Finished, 0),
            game(2, refused(4), 1),
            game(
                3,
                Ended::Masked {
                    at: 83,
                    stop: StopKind::Refused,
                    said: "refuses \"x\"".into(),
                    diverged_at: 85,
                    diverged: "Discards { log: 1, engine: 4 }".into(),
                },
                1,
            ),
        ]);
        let json = facet_json::to_string(&r).unwrap();
        assert_eq!(facet_json::from_str::<Run>(&json).unwrap(), r, "{json}");
    }
}
