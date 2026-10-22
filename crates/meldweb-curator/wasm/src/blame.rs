//! Blame per card (ADR-0041): for each card in a deck as a commit (or the
//! editor's unsaved text) has it, the commit that last changed that card and
//! the one it came into the deck in.
//!
//! A card is followed by identity, not by text line: two versions' cards are
//! the same card when `chip-decklist`'s diff pairs them, the pairing every
//! commit message is written from. So a card keeps its blame when the lines
//! around it move, when its comment is rewritten, or when another card is
//! added above it.
//!
//! **What changes a card** is what the diff says about its line: its quantity,
//! its categories (moving it to the sideboard or maybeboard is a category
//! change), its printing and its finish. Coming into the deck starts it over:
//! a card removed and added again was added at the second add. What the diff
//! says about the deck and not a card (its name, format, cover, description,
//! a category declared or retyped) changes no card, nor does reordering lines
//! or editing a comment.
//!
//! **The walk** is `git blame`'s, over the deck's whole ancestry. Each card is
//! a suspect at one commit at a time, newest commit first. At a commit whose
//! card is the same as in some parent, the suspect passes to the first such
//! parent; where it differs from every parent it was changed there; where no
//! parent has it, it was added there. So a change brought in by a merge is
//! blamed on the commit on the side where it was made. A two-parent merge is
//! blamed itself only where it settled the card: when one side changed the
//! card since the merge base and the merge does not hold that side's card, as
//! after keeping the other side of a clash. A fork's line runs into the deck
//! it was forked from, whose file has the other deck's stem, so where a
//! parent lacks the deck's file the walk follows the one deck file that
//! parent holds: a card unchanged since the fork is the original's.
//!
//! After its last change a card is followed on to where it was added, through
//! any parent that still holds it, changed or not.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::rc::Rc;

use chip_decklist::deck::{Card, CardRef, Deck, Printing};
use chip_decklist::diff::{pair_lines, Change, Diff};
use chip_decklist::stem::DeckStem;
use chip_decklist::versions::Versions;
use facet::Facet;
use meldweb_git::{BlobId, CommitId, CommitInfo, EntryKind, Error, Repo, RepoPath, Store};

type Out<T> = Result<T, String>;

/// One commit, as a card's blame names it.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[facet(rename_all = "camelCase")]
pub struct BlameCommit {
    pub commit: String,
    /// The stem of the deck the card was in at that commit: this deck's, or
    /// for a card unchanged since a fork, the deck it was forked from.
    pub deck: String,
    pub author: String,
    pub email: String,
    /// The author's time, in seconds since the Unix epoch, as `git blame`
    /// dates a line.
    pub authored_at: i64,
    /// Without the newline git ends a message with.
    pub message: String,
}

/// When a card came into the deck and when it last changed.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Blame {
    /// Added by edits not saved yet.
    AddedUnsaved,
    /// Came in at `added`, and changed since by edits not saved yet.
    ChangedUnsaved { added: BlameCommit },
    /// Came in at `added` as it is now.
    Added { added: BlameCommit },
    /// Came in at `added`, and last changed at `changed`.
    Changed {
        added: BlameCommit,
        changed: BlameCommit,
    },
}

/// One card line of the deck blamed.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct CardBlame {
    /// The line's index among the deck's cards.
    pub index: u32,
    /// What its last change did to it, as that commit's message says it:
    /// `+1 Sol Ring`, `Sol Ring: 1 → 2`.
    pub changes: Vec<String>,
    pub blame: Blame,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum DeckBlame {
    /// `at` is the commit blamed; `cards` follow the deck's card lines, as
    /// the unsaved text has them when there was one.
    Blamed { at: String, cards: Vec<CardBlame> },
    /// No such commit, or none holding a deck Progress Engine reads.
    NoDeck,
}

/// Where in history a card was found to change or come in: a commit this
/// walk dug, or one an earlier blame settled.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Found {
    At(CommitId),
    Known(BlameCommit),
}

/// What blame settled for one card line of a commit's deck.
#[derive(Debug, Clone)]
struct Settled {
    changed: BlameCommit,
    changes: Vec<String>,
    added: BlameCommit,
}

/// What earlier blames settled, by the commit blamed. A commit never
/// changes, so neither does its blame, and blaming a deck again after a save
/// digs only the commits since the last blame.
#[derive(Debug, Default)]
pub struct Memo {
    at: HashMap<CommitId, Rc<Vec<Settled>>>,
    /// The deck versions read so far, each card line once.
    reader: Versions,
}

/// Commits whose blame [`Memo`] keeps before it starts again.
const REMEMBERED: usize = 256;

/// One version of the deck's file, parsed once.
struct Rev {
    deck: Deck,
    names: HashMap<Printing, String>,
}

impl Rev {
    fn read(versions: &mut Versions, text: &str) -> Option<Self> {
        let (deck, comments) = versions.read(text).ok()?;
        let names = deck
            .cards
            .iter()
            .zip(comments)
            .filter_map(|(c, comment)| match (&c.card, comment) {
                (CardRef::Printing(p), Some(name)) => Some((p.clone(), name)),
                _ => None,
            })
            .collect();
        Some(Rev { deck, names })
    }
}

/// A version of the deck file and the blob it is, so that two equal blobs
/// pair without being compared.
#[derive(Clone)]
struct Version {
    path: RepoPath,
    blob: Option<BlobId>,
    rev: Rc<Rev>,
}

/// How a parent's card lines map onto a child's: for each child line, the
/// parent line that is the same card, and whether it is unchanged.
type Links = HashMap<usize, (usize, bool)>;

/// A two-parent merge's merge base, and how each parent's cards pair with
/// its own.
struct Base {
    version: Version,
    links: Vec<Option<Rc<Links>>>,
}

/// A card being blamed, at the version where it is still a suspect.
#[derive(Debug, Clone, Copy)]
struct Suspect {
    /// The card's place in the answer.
    card: usize,
    /// Its line in the version it is at.
    line: usize,
}

fn same(a: &Card, b: &Card) -> bool {
    a.qty == b.qty && a.categories == b.categories && a.card == b.card && a.finish == b.finish
}

fn said(e: Error) -> String {
    e.to_string()
}

struct Walk<'r, S: Store> {
    repo: &'r Repo<S>,
    reader: Versions,
    commits: HashMap<CommitId, CommitInfo>,
    versions: HashMap<CommitId, Option<Version>>,
    revs: HashMap<BlobId, Option<Rc<Rev>>>,
    links: HashMap<(BlobId, BlobId), Rc<Links>>,
    memo: &'r Memo,
    /// Per card: the last change, with what it did, and where it came in.
    last: Vec<Option<(Found, Vec<String>)>>,
    added: Vec<Option<Found>>,
}

impl<'r, S: Store> Walk<'r, S> {
    fn commit(&mut self, id: &CommitId) -> Out<&CommitInfo> {
        if !self.commits.contains_key(id) {
            let info = self.repo.read_commit(id).map_err(said)?;
            self.commits.insert(*id, info);
        }
        Ok(&self.commits[id])
    }

    fn rev(&mut self, blob: BlobId) -> Out<Option<Rc<Rev>>> {
        if let Some(r) = self.revs.get(&blob) {
            return Ok(r.clone());
        }
        let bytes = self.repo.read_blob(&blob).map_err(said)?;
        // A version that does not read as a deck is where the walk stops: its
        // cards are taken to have come in at the version after it.
        let rev = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|text| Rev::read(&mut self.reader, text))
            .map(Rc::new);
        self.revs.insert(blob, rev.clone());
        Ok(rev)
    }

    fn file(&mut self, at: &CommitId, path: &RepoPath) -> Out<Option<BlobId>> {
        let tree = self.commit(at)?.tree;
        match self.repo.entry_in(tree, path).map_err(said)? {
            Some(EntryKind::File(b) | EntryKind::Executable(b)) => Ok(Some(b)),
            _ => Ok(None),
        }
    }

    /// The deck file at `at`: at `path`, or else the one deck file there,
    /// which is the deck a fork was forked from.
    fn version(&mut self, at: &CommitId, path: &RepoPath) -> Out<Option<Version>> {
        if let Some(v) = self.versions.get(at) {
            return Ok(v.clone());
        }
        let found = match self.file(at, path)? {
            Some(blob) => Some((path.clone(), blob)),
            None => {
                let entries = self.repo.list_dir(at, None).map_err(said)?;
                let mut decks = entries.into_iter().flatten().filter_map(|e| match e.kind {
                    EntryKind::File(b) | EntryKind::Executable(b)
                        if e.name.ends_with(".deck.toml") =>
                    {
                        Some((e.name, b))
                    }
                    _ => None,
                });
                match (decks.next(), decks.next()) {
                    (Some((name, blob)), None) => {
                        Some((RepoPath::new(name).map_err(|e| e.to_string())?, blob))
                    }
                    _ => None,
                }
            }
        };
        let version = match found {
            None => None,
            Some((path, blob)) => self.rev(blob)?.map(|rev| Version {
                path,
                blob: Some(blob),
                rev,
            }),
        };
        self.versions.insert(*at, version.clone());
        Ok(version)
    }

    fn links(&mut self, parent: &Version, child: &Version) -> Rc<Links> {
        let make = || -> Links {
            let names = |p: &Printing| {
                child
                    .rev
                    .names
                    .get(p)
                    .or_else(|| parent.rev.names.get(p))
                    .cloned()
            };
            let (before, after) = (&parent.rev.deck, &child.rev.deck);
            if parent.blob.is_some() && parent.blob == child.blob {
                return (0..after.cards.len()).map(|i| (i, (i, true))).collect();
            }
            pair_lines(before, after, names)
                .into_iter()
                .map(|(b, a)| (a, (b, same(&before.cards[b], &after.cards[a]))))
                .collect()
        };
        match (parent.blob, child.blob) {
            (Some(p), Some(c)) => self
                .links
                .entry((p, c))
                .or_insert_with(|| Rc::new(make()))
                .clone(),
            _ => Rc::new(make()),
        }
    }
}

/// What the change from `parent` to `child` did to the child's `line`, in
/// the commit message's words; from nothing, it was added.
fn changes_of(parent: Option<&Version>, child: &Version, line: usize) -> Vec<String> {
    let card = &child.rev.deck.cards[line];
    let Some(parent) = parent else {
        let label = match &card.card {
            CardRef::Name(n) => n.clone(),
            CardRef::Printing(p) => child
                .rev
                .names
                .get(p)
                .cloned()
                .unwrap_or_else(|| p.to_string()),
        };
        return vec![format!("+{} {label}", card.qty)];
    };
    let names = |p: &Printing| {
        child
            .rev
            .names
            .get(p)
            .or_else(|| parent.rev.names.get(p))
            .cloned()
    };
    Diff::new(&parent.rev.deck, &child.rev.deck, names)
        .changes
        .into_iter()
        .filter(|(c, _)| match c {
            Change::Add { after }
            | Change::Qty { after, .. }
            | Change::Move { after, .. }
            | Change::Printing { after, .. }
            | Change::Finish { after, .. } => *after == line,
            _ => false,
        })
        .map(|(_, text)| text)
        .collect()
}

impl<'r, S: Store> Walk<'r, S> {
    /// Settles what commit `at`, whose deck is `child`, says about each
    /// suspect there, and says which parent each one still unsettled goes on
    /// to, at which line.
    fn step(
        &mut self,
        at: CommitId,
        child: &Version,
        parents: &[(CommitId, Option<Version>)],
        suspects: Vec<Suspect>,
    ) -> Out<Vec<(CommitId, Suspect)>> {
        let links: Vec<Option<Rc<Links>>> = parents
            .iter()
            .map(|(_, v)| v.as_ref().map(|v| self.links(v, child)))
            .collect();
        // A two-parent merge's base, read only once a card asks for it.
        let mut base: Option<Option<Base>> = None;
        let mut next = Vec::new();
        for s in suspects {
            let found: Vec<Option<(usize, bool)>> = links
                .iter()
                .map(|l| l.as_ref().and_then(|l| l.get(&s.line).copied()))
                .collect();
            let unchanged = found.iter().position(|f| matches!(f, Some((_, true))));
            let held = found.iter().position(Option::is_some);
            if self.last[s.card].is_none() {
                let settled_here = match unchanged {
                    None => true,
                    // Unchanged from one side and changed from the other: the
                    // merge may have dropped that side's change.
                    Some(_) if found.iter().any(|f| matches!(f, Some((_, false)))) => {
                        if base.is_none() {
                            base = Some(self.base(parents, child)?);
                        }
                        match base.as_ref().and_then(Option::as_ref) {
                            Some(b) => {
                                dropped_a_side(child, s.line, parents, &b.version, &b.links, &found)
                            }
                            None => false,
                        }
                    }
                    Some(_) => false,
                };
                if settled_here {
                    let from = held.or(if parents.is_empty() { None } else { Some(0) });
                    let parent = from.and_then(|p| parents[p].1.as_ref());
                    let said = changes_of(parent, child, s.line);
                    self.last[s.card] = Some((Found::At(at), said));
                }
            }
            match unchanged.or(held) {
                Some(p) => {
                    let (line, _) = found[p].expect("a parent that holds the card");
                    next.push((parents[p].0, Suspect { card: s.card, line }));
                }
                None => self.added[s.card] = Some(Found::At(at)),
            }
        }
        Ok(next)
    }

    /// A two-parent merge's merge base, with how each parent's cards pair
    /// with its; `None` for any other commit, or a base without the deck.
    fn base(
        &mut self,
        parents: &[(CommitId, Option<Version>)],
        child: &Version,
    ) -> Out<Option<Base>> {
        let [(a, Some(_)), (b, Some(_))] = parents else {
            return Ok(None);
        };
        let Some(m) = self.merge_base(*a, *b)? else {
            return Ok(None);
        };
        let Some(base) = self.version(&m, &child.path)? else {
            return Ok(None);
        };
        let links = parents
            .iter()
            .map(|(_, v)| v.as_ref().map(|v| self.links(&base, v)))
            .collect();
        Ok(Some(Base {
            version: base,
            links,
        }))
    }

    /// The newest commit both `a` and `b` descend from, by committer time:
    /// a merge base, as `git merge-base` finds one, through the commits this
    /// walk has mostly read already.
    fn merge_base(&mut self, a: CommitId, b: CommitId) -> Out<Option<CommitId>> {
        let mut of_a = HashSet::new();
        let mut stack = vec![a];
        while let Some(c) = stack.pop() {
            if of_a.insert(c) {
                stack.extend(self.commit(&c)?.parents.iter().copied());
            }
        }
        let mut seen = HashSet::new();
        let mut queue = BinaryHeap::new();
        queue.push((self.commit(&b)?.committer.when.seconds(), b));
        while let Some((_, c)) = queue.pop() {
            if of_a.contains(&c) {
                return Ok(Some(c));
            }
            if !seen.insert(c) {
                continue;
            }
            for p in self.commit(&c)?.parents.clone() {
                let when = self.commit(&p)?.committer.when.seconds();
                queue.push((when, p));
            }
        }
        Ok(None)
    }

    fn facts(&mut self, id: &CommitId) -> Out<BlameCommit> {
        let deck = match self.versions.get(id) {
            Some(Some(v)) => v.path.as_str().trim_end_matches(".deck.toml").to_owned(),
            _ => return Err(format!("{id} was blamed without a deck")),
        };
        let info = self.commit(id)?;
        Ok(BlameCommit {
            commit: id.to_hex(),
            deck,
            author: info.author.who.name().to_owned(),
            email: info.author.who.email().to_owned(),
            authored_at: info.author.when.seconds(),
            message: info.message.trim_end_matches('\n').to_owned(),
        })
    }
}

/// Whether a two-parent merge settled the card on `line` itself: one side
/// changed it since the merge base, and the merge holds something else.
fn dropped_a_side(
    child: &Version,
    line: usize,
    parents: &[(CommitId, Option<Version>)],
    base: &Version,
    base_links: &[Option<Rc<Links>>],
    found: &[Option<(usize, bool)>],
) -> bool {
    let now = &child.rev.deck.cards[line];
    parents.iter().enumerate().any(|(p, (_, v))| {
        let (Some(v), Some((k, false))) = (v, found[p]) else {
            return false;
        };
        let theirs = &v.rev.deck.cards[k];
        let was = base_links[p]
            .as_ref()
            .and_then(|l| l.get(&k))
            .map(|&(b, _)| &base.rev.deck.cards[b]);
        let side_changed = !was.is_some_and(|was| same(was, theirs));
        side_changed && !same(theirs, now)
    })
}

/// Every card of `stem`'s deck at `at` blamed, or of `unsaved` when given:
/// the editor's text, on top of `at`.
pub fn blame<S: Store>(
    repo: &Repo<S>,
    memo: &mut Memo,
    stem: &DeckStem,
    at: CommitId,
    unsaved: Option<&str>,
) -> Out<DeckBlame> {
    let path = RepoPath::new(stem.deck_file()).map_err(|e| e.to_string())?;
    let mut reader = std::mem::take(&mut memo.reader);
    let saved = blame_commit(repo, memo, &mut reader, &path, at);
    memo.reader = reader;
    let Some(saved) = saved? else {
        return Ok(DeckBlame::NoDeck);
    };
    let (version, settled) = saved;
    let settled_card = |i: usize, s: &Settled| CardBlame {
        index: i as u32,
        changes: s.changes.clone(),
        blame: if s.added.commit == s.changed.commit {
            Blame::Added {
                added: s.added.clone(),
            }
        } else {
            Blame::Changed {
                added: s.added.clone(),
                changed: s.changed.clone(),
            }
        },
    };
    // Unsaved text that reads as a deck and differs from the commit is one
    // more version on top of it, as `git blame` shows "Not Committed Yet".
    let unsaved = unsaved
        .and_then(|text| Rev::read(&mut memo.reader, text))
        .filter(|r| r.deck != version.rev.deck);
    let cards = match unsaved {
        None => settled
            .iter()
            .enumerate()
            .map(|(i, s)| settled_card(i, s))
            .collect(),
        Some(rev) => {
            let top = Version {
                path: version.path.clone(),
                blob: None,
                rev: Rc::new(rev),
            };
            let names = |p: &Printing| {
                top.rev
                    .names
                    .get(p)
                    .or_else(|| version.rev.names.get(p))
                    .cloned()
            };
            let pairs: HashMap<usize, usize> = pair_lines(&version.rev.deck, &top.rev.deck, names)
                .into_iter()
                .map(|(b, a)| (a, b))
                .collect();
            (0..top.rev.deck.cards.len())
                .map(|i| match pairs.get(&i) {
                    None => CardBlame {
                        index: i as u32,
                        changes: changes_of(Some(&version), &top, i),
                        blame: Blame::AddedUnsaved,
                    },
                    Some(&k) if same(&version.rev.deck.cards[k], &top.rev.deck.cards[i]) => {
                        settled_card(i, &settled[k])
                    }
                    Some(&k) => CardBlame {
                        index: i as u32,
                        changes: changes_of(Some(&version), &top, i),
                        blame: Blame::ChangedUnsaved {
                            added: settled[k].added.clone(),
                        },
                    },
                })
                .collect()
        }
    };
    Ok(DeckBlame::Blamed {
        at: at.to_hex(),
        cards,
    })
}

/// The commits with suspects at them, newest first, as `git blame` digs. A
/// commit reached again after it was dug (clocks disagree) is dug again for
/// the suspects that came late, since each suspect is settled on its own.
#[derive(Default)]
struct Dig {
    pending: HashMap<CommitId, Vec<Suspect>>,
    queue: BinaryHeap<(i64, Reverse<u64>, CommitId)>,
    order: u64,
}

impl Dig {
    fn send<S: Store>(&mut self, walk: &mut Walk<S>, c: CommitId, s: Suspect) -> Out<()> {
        // A commit an earlier blame settled answers for its cards at once.
        if let Some(known) = walk.memo.at.get(&c) {
            let k = &known[s.line];
            if walk.last[s.card].is_none() {
                walk.last[s.card] = Some((Found::Known(k.changed.clone()), k.changes.clone()));
            }
            walk.added[s.card] = Some(Found::Known(k.added.clone()));
            return Ok(());
        }
        let list = self.pending.entry(c).or_default();
        if list.is_empty() {
            let when = walk.commit(&c)?.committer.when.seconds();
            self.order += 1;
            self.queue.push((when, Reverse(self.order), c));
        }
        list.push(s);
        Ok(())
    }

    fn next(&mut self) -> Option<(CommitId, Vec<Suspect>)> {
        while let Some((_, _, c)) = self.queue.pop() {
            if let Some(suspects) = self.pending.remove(&c).filter(|s| !s.is_empty()) {
                return Some((c, suspects));
            }
        }
        None
    }
}

/// Every card line of the deck at commit `at` settled, from `memo` or by
/// digging; `None` when `at` holds no deck.
fn blame_commit<S: Store>(
    repo: &Repo<S>,
    memo: &mut Memo,
    reader: &mut Versions,
    path: &RepoPath,
    at: CommitId,
) -> Out<Option<(Version, Rc<Vec<Settled>>)>> {
    let mut walk = Walk {
        repo,
        reader: std::mem::take(reader),
        memo,
        commits: HashMap::new(),
        versions: HashMap::new(),
        revs: HashMap::new(),
        links: HashMap::new(),
        last: vec![],
        added: vec![],
    };
    // The deck blamed is the one at `path`; only a parent may hold another's.
    let Some(top) = walk.version(&at, path)?.filter(|v| v.path == *path) else {
        *reader = walk.reader;
        return Ok(None);
    };
    if let Some(known) = walk.memo.at.get(&at) {
        let known = known.clone();
        *reader = walk.reader;
        return Ok(Some((top, known)));
    }
    let count = top.rev.deck.cards.len();
    walk.last = vec![None; count];
    walk.added = vec![None; count];

    let mut dig = Dig::default();
    for i in 0..count {
        dig.send(&mut walk, at, Suspect { card: i, line: i })?;
    }
    while let Some((c, suspects)) = dig.next() {
        let child = walk
            .version(&c, path)?
            .expect("a suspect is only sent to a version that holds the deck");
        let parent_ids = walk.commit(&c)?.parents.clone();
        let mut parents = Vec::with_capacity(parent_ids.len());
        for p in parent_ids {
            let v = walk.version(&p, &child.path)?;
            parents.push((p, v));
        }
        for (p, s) in walk.step(c, &child, &parents, suspects)? {
            dig.send(&mut walk, p, s)?;
        }
    }

    let mut facts: HashMap<CommitId, BlameCommit> = HashMap::new();
    let mut fact = |walk: &mut Walk<S>, found: Found| -> Out<BlameCommit> {
        let id = match found {
            Found::Known(f) => return Ok(f),
            Found::At(id) => id,
        };
        if let Some(f) = facts.get(&id) {
            return Ok(f.clone());
        }
        let f = walk.facts(&id)?;
        facts.insert(id, f.clone());
        Ok(f)
    };
    let mut settled = Vec::with_capacity(count);
    for i in 0..count {
        let (changed, changes) = walk.last[i]
            .take()
            .ok_or_else(|| format!("card {i} was never settled"))?;
        let added = walk.added[i]
            .take()
            .ok_or_else(|| format!("card {i} never came in"))?;
        settled.push(Settled {
            changed: fact(&mut walk, changed)?,
            changes,
            added: fact(&mut walk, added)?,
        });
    }
    let settled = Rc::new(settled);
    *reader = walk.reader;
    if memo.at.len() >= REMEMBERED {
        memo.at.clear();
    }
    memo.at.insert(at, settled.clone());
    Ok(Some((top, settled)))
}
