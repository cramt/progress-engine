//! Three-way merges: of two texts by lines, and of two commits' trees path
//! by path from their merge base, as a sync merges what this browser and
//! the remote each committed (ADR-0041).
//!
//! The line merge is the one Curator settled a refused save with
//! (ADR-0021). It began as `web/src/github/merge.ts`; the page now calls
//! this one through the wasm, so a save and a sync merge alike.

use std::collections::BTreeMap;

use gix_hash::ObjectId;
use gix_object::bstr::BString;
use gix_object::tree::{EntryKind as GixEntryKind, EntryMode};

use crate::names::{CommitId, TreeId};
use crate::repo::{Dir, Node, Repo, Result};
use crate::store::Store;

/// `ours` and `theirs` both made from `base`, with each one's changes kept,
/// or `None` when they changed the same lines differently.
///
/// Decks and the collection hold one card per line, so two edits usually
/// touch different lines; where both changed one stretch differently it
/// gives up rather than guess.
pub fn merge_lines(base: &str, ours: &str, theirs: &str) -> Option<String> {
    merge_lines_with(base, ours, theirs, None).ok()
}

/// One stretch of lines both sides changed from `base`, differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub base: Vec<String>,
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
}

/// [`merge_lines`], with what stops it: every stretch both sides changed
/// differently. With `prefer`, each such stretch takes that side's lines
/// and the rest of both sides' changes are kept, so the merge always
/// succeeds; whether what it makes still reads is the caller's to say.
pub fn merge_lines_with(
    base: &str,
    ours: &str,
    theirs: &str,
    prefer: Option<Side>,
) -> std::result::Result<String, Vec<Hunk>> {
    if ours == theirs || theirs == base {
        return Ok(ours.to_owned());
    }
    if ours == base {
        return Ok(theirs.to_owned());
    }
    let b: Vec<&str> = base.split('\n').collect();
    let o: Vec<&str> = ours.split('\n').collect();
    let t: Vec<&str> = theirs.split('\n').collect();
    let in_ours = matching(&b, &o);
    let in_theirs = matching(&b, &t);

    // The base lines both sides kept; between two of them is one hunk. The
    // ends of the texts close the last one.
    let mut kept: Vec<(usize, usize, usize)> = in_ours
        .iter()
        .enumerate()
        .filter_map(|(bi, oi)| Some((bi, (*oi)?, in_theirs[bi]?)))
        .collect();
    kept.push((b.len(), o.len(), t.len()));

    let mut out: Vec<&str> = Vec::new();
    let mut hunks = Vec::new();
    let (mut i, mut j, mut k) = (0, 0, 0);
    for (bi, oi, ti) in kept {
        let was = &b[i..bi];
        let mine = &o[j..oi];
        let yours = &t[k..ti];
        if mine == was {
            out.extend_from_slice(yours);
        } else if yours == was || mine == yours {
            out.extend_from_slice(mine);
        } else {
            match prefer {
                Some(Side::Ours) => out.extend_from_slice(mine),
                Some(Side::Theirs) => out.extend_from_slice(yours),
                None => {
                    let owned = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect();
                    hunks.push(Hunk {
                        base: owned(was),
                        ours: owned(mine),
                        theirs: owned(yours),
                    });
                }
            }
        }
        out.extend(b.get(bi).copied());
        (i, j, k) = (bi + 1, oi + 1, ti + 1);
    }
    if hunks.is_empty() {
        Ok(out.join("\n"))
    } else {
        Err(hunks)
    }
}

/// For each line of `a`, the line of `b` a longest common subsequence pairs
/// it with. The shared head and tail are paired first, so the table is only
/// as big as the stretch that changed.
fn matching(a: &[&str], b: &[&str]) -> Vec<Option<usize>> {
    let mut pair = vec![None; a.len()];
    let mut head = 0;
    while head < a.len() && head < b.len() && a[head] == b[head] {
        pair[head] = Some(head);
        head += 1;
    }
    let mut tail = 0;
    while tail < a.len() - head
        && tail < b.len() - head
        && a[a.len() - 1 - tail] == b[b.len() - 1 - tail]
    {
        pair[a.len() - 1 - tail] = Some(b.len() - 1 - tail);
        tail += 1;
    }
    let n = a.len() - head - tail;
    let m = b.len() - head - tail;
    // run(x, y): the longest common subsequence of a[head+x..] and b[head+y..].
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |x: usize, y: usize| x * (m + 1) + y;
    for x in (0..n).rev() {
        for y in (0..m).rev() {
            lcs[at(x, y)] = if a[head + x] == b[head + y] {
                lcs[at(x + 1, y + 1)] + 1
            } else {
                lcs[at(x + 1, y)].max(lcs[at(x, y + 1)])
            };
        }
    }
    let (mut x, mut y) = (0, 0);
    while x < n && y < m {
        if a[head + x] == b[head + y] {
            pair[head + x] = Some(head + y);
            x += 1;
            y += 1;
        } else if lcs[at(x + 1, y)] >= lcs[at(x, y + 1)] {
            x += 1;
        } else {
            y += 1;
        }
    }
    pair
}

/// Which side a conflicting path takes when the user has chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// This repo's version.
    Ours,
    /// The other side's version.
    Theirs,
}

/// What merging two commits' trees came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeMerge {
    /// The merged tree, and the paths that took the other side's changes.
    Clean { tree: TreeId, taken: Vec<String> },
    /// Paths both sides changed in a way that does not merge.
    Conflicts(Vec<String>),
}

/// What merging one commit into a branch would do; see [`Repo::plan_merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planned {
    /// The branch already holds every commit of the other.
    UpToDate,
    /// The branch can move to the other commit as it is.
    FastForward,
    /// A merge commit of this tree; `taken` are the paths that took the
    /// other side's changes.
    Merge { tree: TreeId, taken: Vec<String> },
    /// Paths both sides changed in a way that does not merge.
    Conflicts(Vec<String>),
    /// No common ancestor.
    Unrelated,
}

type Leaf = (EntryMode, ObjectId);

/// Merges one file both sides changed: `(path, base, ours, theirs)` to the
/// merged bytes, or `None` when they do not merge.
pub type MergeFile<'a> = dyn FnMut(&str, &[u8], &[u8], &[u8]) -> Option<Vec<u8>> + 'a;

impl<S: Store> Repo<S> {
    fn flatten(&self, tree: TreeId, prefix: &str, out: &mut BTreeMap<String, Leaf>) -> Result<()> {
        for (name, mode, id) in self.tree_entries(&tree)? {
            let path = format!("{prefix}{name}");
            if mode.is_tree() {
                self.flatten(TreeId(id), &format!("{path}/"), out)?;
            } else {
                out.insert(path, (mode, id));
            }
        }
        Ok(())
    }

    fn files(&self, commit: Option<&CommitId>) -> Result<BTreeMap<String, Leaf>> {
        let mut out = BTreeMap::new();
        if let Some(c) = commit {
            self.flatten(self.tree_of(c)?, "", &mut out)?;
        }
        Ok(out)
    }

    /// Merges `theirs` into `ours` from `base` (none for unrelated
    /// histories), path by path: a path one side changed takes that side; a
    /// file both changed goes to `merge_file(path, base, ours, theirs)`,
    /// which gives the merged bytes or `None` when they do not merge. A
    /// conflicting path takes `prefer`'s side when there is one.
    pub fn merge_trees(
        &mut self,
        base: Option<&CommitId>,
        ours: &CommitId,
        theirs: &CommitId,
        prefer: Option<Side>,
        merge_file: &mut MergeFile<'_>,
    ) -> Result<TreeMerge> {
        let b = self.files(base)?;
        let o = self.files(Some(ours))?;
        let t = self.files(Some(theirs))?;
        let mut result = o.clone();
        let mut taken = Vec::new();
        let mut conflicts = Vec::new();
        let mut paths: Vec<&String> = o.keys().chain(t.keys()).chain(b.keys()).collect();
        paths.sort();
        paths.dedup();
        let regular = |l: &Leaf| {
            matches!(
                GixEntryKind::from(l.0),
                GixEntryKind::Blob | GixEntryKind::BlobExecutable
            )
        };
        for path in paths {
            let (bl, ol, tl) = (b.get(path), o.get(path), t.get(path));
            if ol == tl || bl == tl {
                continue;
            }
            let take_theirs = |result: &mut BTreeMap<String, Leaf>| match tl {
                Some(leaf) => {
                    result.insert(path.clone(), *leaf);
                }
                None => {
                    result.remove(path);
                }
            };
            if bl == ol {
                take_theirs(&mut result);
                taken.push(path.clone());
                continue;
            }
            let merged = match (ol, tl) {
                (Some(ol), Some(tl)) if regular(ol) && regular(tl) && bl.is_none_or(regular) => {
                    let base_bytes = match bl {
                        Some(l) => self.read_object(&l.1, gix_object::Kind::Blob)?,
                        None => Vec::new(),
                    };
                    let ours_bytes = self.read_object(&ol.1, gix_object::Kind::Blob)?;
                    let theirs_bytes = self.read_object(&tl.1, gix_object::Kind::Blob)?;
                    merge_file(path, &base_bytes, &ours_bytes, &theirs_bytes)
                        .map(|bytes| (ol.0, bytes))
                }
                _ => None,
            };
            match (merged, prefer) {
                (Some((mode, bytes)), _) => {
                    let id = self.write_object(gix_object::Kind::Blob, &bytes)?;
                    result.insert(path.clone(), (mode, id));
                    taken.push(path.clone());
                }
                (None, Some(Side::Ours)) => {}
                (None, Some(Side::Theirs)) => {
                    take_theirs(&mut result);
                    taken.push(path.clone());
                }
                (None, None) => conflicts.push(path.clone()),
            }
        }
        if !conflicts.is_empty() {
            return Ok(TreeMerge::Conflicts(conflicts));
        }
        let tree = self.write_flat(&result)?;
        Ok(TreeMerge::Clean { tree, taken })
    }

    /// Whether `ancestor` is `of` or one of its ancestors, over all parents.
    pub fn is_ancestor(&self, ancestor: &CommitId, of: &CommitId) -> Result<bool> {
        let mut cache = Default::default();
        Ok(self.ancestors(&[*of], &mut cache)?.contains(ancestor))
    }

    /// How many commits `a` has that `b` does not, over all parents, as
    /// `git rev-list --count b..a` counts them.
    pub fn count_only_in(&self, a: &CommitId, b: &CommitId) -> Result<usize> {
        let mut cache = Default::default();
        let of_a = self.ancestors(&[*a], &mut cache)?;
        let of_b = self.ancestors(&[*b], &mut cache)?;
        Ok(of_a.difference(&of_b).count())
    }

    /// What merging the commit `from` into a branch at `into` comes to, with
    /// no ref moved: nothing when `into` already holds `from`, a
    /// fast-forward when `into` is an ancestor of `from` (the branch merged
    /// into has not moved since), and otherwise [`Repo::merge_trees`] from
    /// the merge base, whose tree a merge commit with the parents `into`
    /// then `from` would hold. Unrelated histories do not merge.
    pub fn plan_merge(
        &mut self,
        into: &CommitId,
        from: &CommitId,
        prefer: Option<Side>,
        merge_file: &mut MergeFile<'_>,
    ) -> Result<Planned> {
        if self.is_ancestor(from, into)? {
            return Ok(Planned::UpToDate);
        }
        if self.is_ancestor(into, from)? {
            return Ok(Planned::FastForward);
        }
        let bases = self.merge_base(into, from)?;
        let Some(base) = bases.first() else {
            return Ok(Planned::Unrelated);
        };
        Ok(
            match self.merge_trees(Some(base), into, from, prefer, merge_file)? {
                TreeMerge::Conflicts(paths) => Planned::Conflicts(paths),
                TreeMerge::Clean { tree, taken } => Planned::Merge { tree, taken },
            },
        )
    }

    /// A tree of exactly these files, by path.
    pub(crate) fn write_flat(&mut self, files: &BTreeMap<String, Leaf>) -> Result<TreeId> {
        let mut root: BTreeMap<BString, Node> = BTreeMap::new();
        for (path, (mode, id)) in files {
            let mut parts: Vec<&str> = path.split('/').collect();
            let name = parts.pop().expect("a path has a name");
            let mut dir = &mut root;
            for part in parts {
                let node = dir
                    .entry(BString::from(part))
                    .or_insert_with(|| Node::Dir(Dir::Edited(BTreeMap::new())));
                dir = match node {
                    Node::Dir(Dir::Edited(entries)) => entries,
                    _ => unreachable!("a path's directories are directories"),
                };
            }
            dir.insert(
                BString::from(name),
                Node::Leaf {
                    mode: *mode,
                    id: *id,
                },
            );
        }
        self.write_dir(Dir::Edited(root))
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_lines, merge_lines_with, Hunk, Side};

    fn lines(l: &[&str]) -> String {
        format!("{}\n", l.join("\n"))
    }

    #[test]
    fn different_lines_merge_and_the_same_lines_do_not() {
        let base = lines(&["cards = [", "  a,", "  b,", "  c,", "  d,", "]"]);
        let ours = lines(&["cards = [", "  A,", "  b,", "  c,", "  d,", "]"]);
        let theirs = lines(&["cards = [", "  a,", "  b,", "  c,", "  D,", "]"]);
        assert_eq!(
            merge_lines(&base, &ours, &theirs).unwrap(),
            lines(&["cards = [", "  A,", "  b,", "  c,", "  D,", "]"])
        );
        let clash = lines(&["cards = [", "  X,", "  b,", "  c,", "  d,", "]"]);
        assert_eq!(merge_lines(&base, &ours, &clash), None);
    }

    #[test]
    fn a_clash_names_its_stretch_and_a_side_settles_only_that_stretch() {
        let base = lines(&["a", "b", "c", "d", "e", "f"]);
        let ours = lines(&["A", "b", "c", "d1", "e", "f"]);
        let theirs = lines(&["a", "b", "c", "d2", "e", "F"]);
        assert_eq!(
            merge_lines_with(&base, &ours, &theirs, None),
            Err(vec![Hunk {
                base: vec!["d".into()],
                ours: vec!["d1".into()],
                theirs: vec!["d2".into()],
            }])
        );
        // Both sides' other changes are kept; only the clash takes a side.
        assert_eq!(
            merge_lines_with(&base, &ours, &theirs, Some(Side::Ours)),
            Ok(lines(&["A", "b", "c", "d1", "e", "F"]))
        );
        assert_eq!(
            merge_lines_with(&base, &ours, &theirs, Some(Side::Theirs)),
            Ok(lines(&["A", "b", "c", "d2", "e", "F"]))
        );
    }
}
