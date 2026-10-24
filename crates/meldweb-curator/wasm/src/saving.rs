//! A save (ADR-0021): the editor's text committed in one call, under the
//! repo's lock. The save names the text it was made from; when the file has
//! not moved since, the text is committed, and when it has, the save either
//! stops or merges, as the file's kind merges ([`merging::merge_bytes`]: a
//! deck by card, anything else by lines), and commits the merge. Nothing can
//! land between reading the file and writing it, so a save never retries.
//!
//! A merge that stops is a [`Conflict`] guarded by the three texts it was
//! worked out from ([`Guard::Blobs`]): picks made against it settle only a
//! save of the same text, from the same text, over the same file.
//!
//! What the commit says is the file kind's own diff ([`message_for`]),
//! which also says whether a merge left a file Progress Engine reads.

use chip_decklist::{changelog, collection, wanted};
use facet::Facet;
use meldweb_git::{
    BlobId, BranchName, Change, EntryKind, Error, Repo, RepoPath, Signature, Store, Tip,
    WriteCommit,
};

use crate::merging::{self, Conflict, Guard, Settle, Settling, Stops};
use crate::preference;

type Out<T> = Result<T, String>;

/// What to do when the file moved since the save's text was made.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Moved {
    /// Stop, and say so: an editor that does not take merges.
    Refuse,
    /// Merge the save into the file as it is now, settled by `settle` when
    /// it was made against this very merge.
    Merge {
        #[facet(default)]
        settle: Option<Settle>,
    },
}

/// What a save's text was made from.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Made {
    /// The file as it was read, `base` its text; absent, there was no file
    /// and there must still be none.
    From {
        #[facet(default)]
        base: Option<String>,
        moved: Moved,
    },
    /// Nothing in particular: it replaces whatever the file is now, as the
    /// user chose over a conflict.
    Over,
}

/// One save: `text` at `path` on `branch` (the default branch when absent).
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct SaveAsk {
    pub path: String,
    #[facet(default)]
    pub branch: Option<String>,
    pub text: String,
    pub made: Made,
}

/// What a save came to.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Saved {
    /// One commit, `commit`, the branch's new tip, holding the file as the
    /// blob `sha`: the save's text, or `merged` when the file had moved and
    /// the save merged into it.
    Committed {
        sha: String,
        commit: String,
        #[facet(default, skip_serializing_if = Option::is_none)]
        merged: Option<String>,
    },
    /// The file already holds what the save would make, `merged` when it
    /// merged, so nothing was committed.
    Unchanged {
        sha: String,
        #[facet(default, skip_serializing_if = Option::is_none)]
        merged: Option<String>,
    },
    /// The file moved and the merge stopped, on what the conflict says.
    Clashed { conflict: Conflict },
    /// The file moved, was made or was deleted since the save's text was
    /// made from it, and this save does not merge into it. Nothing was
    /// committed.
    Moved { message: String },
    /// The save can never commit as asked: not a file Progress Engine
    /// edits, or not one it reads.
    Refused { message: String },
}

/// Why there is no commit message for a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsaid {
    /// Curator writes this path otherwise, if at all.
    NotEdited,
    /// `after` is not a file of the kind its path is, and why.
    Unreadable(String),
}

/// The changelog subject and body for saving `before` as `after` at `path`,
/// from the diff of the file kind the path is. An empty `before` is the
/// file's first save. Criteria and custom cards are written by hand, with
/// no diff to tell, so their message only says which.
pub fn message_for(path: &str, before: &str, after: &str) -> Result<String, Unsaid> {
    let unreadable = |e: &dyn ToString| Unsaid::Unreadable(e.to_string());
    let made = if before.is_empty() { "made" } else { "edited" };
    let own = |suffix: &str| path.strip_suffix(suffix).filter(|s| !s.is_empty());
    match path {
        "collection.toml" => {
            collection::commit_message_for_text(before, after, path).map_err(|e| unreadable(&e))
        }
        "wanted.toml" => {
            wanted::commit_message_for_text(before, after, path).map_err(|e| unreadable(&e))
        }
        "meldweb.toml" => Ok(preference::commit_message(before, after)),
        _ if merging::is_deck(path) => {
            changelog::commit_message_for_text(before, after, path).map_err(|e| unreadable(&e))
        }
        _ => match (
            own(".criteria.toml").filter(|s| !s.contains('/')),
            path.strip_prefix("cards/")
                .and_then(|p| p.strip_suffix(".card.toml"))
                .filter(|s| !s.is_empty() && !s.contains('/')),
        ) {
            (Some(stem), _) => Ok(format!("{stem}: criteria {made}")),
            (_, Some(stem)) => Ok(format!("{stem}: custom card {made}")),
            _ => Err(Unsaid::NotEdited),
        },
    }
}

/// The file as the branch's tip has it.
struct Now {
    blob: BlobId,
    text: String,
}

fn said(e: Error) -> String {
    e.to_string()
}

/// Saves as `ask` says; see the module's docs. The caller holds the repo's
/// lock, so the file read here is the one the commit replaces.
pub fn save<S: Store>(repo: &mut Repo<S>, ask: &SaveAsk, who: &Signature) -> Out<Saved> {
    let path = RepoPath::new(ask.path.as_str()).map_err(|e| e.to_string())?;
    let branch = match &ask.branch {
        Some(b) => BranchName::new(b.as_str()).map_err(|e| e.to_string())?,
        None => repo.head_branch().map_err(said)?,
    };
    let tip = repo.branch_tip(&branch).map_err(said)?;
    let now = match tip {
        None => None,
        Some(t) => match repo.entry(&t, &path).map_err(said)? {
            Some(EntryKind::File(blob) | EntryKind::Executable(blob)) => {
                let bytes = repo.read_blob(&blob).map_err(said)?;
                let text = String::from_utf8_lossy(&bytes).into_owned();
                Some(Now { blob, text })
            }
            Some(_) => return Err(format!("{path} is not a file")),
            None => None,
        },
    };
    let at = Write {
        branch: &branch,
        tip: tip.map_or(Tip::Unborn, Tip::At),
        path: &path,
        who,
    };
    let (base, moved) = match &ask.made {
        Made::Over => {
            let before = now.as_ref().map(|n| n.text.as_str());
            return at.commit(repo, before, &ask.text, None);
        }
        Made::From { base, moved } => (base.as_deref(), moved),
    };
    let base_blob = base
        .map(|b| BlobId::of(b.as_bytes()))
        .transpose()
        .map_err(said)?;
    if now.as_ref().map(|n| n.blob) == base_blob {
        return at.commit(repo, now.as_ref().map(|n| n.text.as_str()), &ask.text, None);
    }
    let settle = match moved {
        Moved::Refuse => {
            return Ok(Saved::Moved {
                message: format!("{path} changed in the repo since it was read"),
            })
        }
        Moved::Merge { settle } => settle.as_ref(),
    };
    let Some(theirs) = now else {
        return Ok(Saved::Moved {
            message: format!("{path} is no longer in the repo"),
        });
    };
    let guard = Guard::Blobs {
        branch: branch.clone(),
        path: path.clone(),
        base: base_blob,
        ours: BlobId::of(ask.text.as_bytes()).map_err(said)?,
        theirs: theirs.blob,
    };
    // Picks made against another merge (the file or the editor's text moved
    // since they were shown) settle nothing, and the clashes are shown anew.
    let settling = Settling::checked(settle, &guard).unwrap_or_default();
    let mut stops = Stops::default();
    let merged = merging::merge_bytes(
        &ask.path,
        base.unwrap_or("").as_bytes(),
        ask.text.as_bytes(),
        theirs.text.as_bytes(),
        &settling,
        &mut stops,
    );
    let Some(merged) = merged else {
        return Ok(Saved::Clashed {
            conflict: Conflict::of(&guard, [ask.path.clone()], &mut stops),
        });
    };
    let merged = String::from_utf8(merged).map_err(|_| format!("{path} merged into no text"))?;
    // What this save changed, over the file as it is now.
    at.commit(repo, Some(&theirs.text), &merged, Some(&merged))
}

/// Where a save commits.
struct Write<'a> {
    branch: &'a BranchName,
    tip: Tip,
    path: &'a RepoPath,
    who: &'a Signature,
}

impl Write<'_> {
    /// `after` committed over `before`, the file at the tip (absent: none);
    /// `merged` is `after` when a merge made it.
    fn commit<S: Store>(
        &self,
        repo: &mut Repo<S>,
        before: Option<&str>,
        after: &str,
        merged: Option<&str>,
    ) -> Out<Saved> {
        let merged = merged.map(str::to_owned);
        let sha = BlobId::of(after.as_bytes()).map_err(said)?.to_hex();
        if before == Some(after) {
            return Ok(Saved::Unchanged { sha, merged });
        }
        let message = match message_for(self.path.as_str(), before.unwrap_or(""), after) {
            Ok(m) => m,
            Err(Unsaid::NotEdited) => {
                return Ok(Saved::Refused {
                    message: format!("{} is not a file Progress Engine edits", self.path),
                })
            }
            Err(Unsaid::Unreadable(why)) => return Ok(Saved::Refused { message: why }),
        };
        let commit = WriteCommit {
            changes: vec![Change::Put {
                path: self.path.clone(),
                bytes: after.as_bytes().to_vec(),
            }],
            message: format!("{}\n", message.trim_end_matches('\n')),
            author: self.who.clone(),
            committer: self.who.clone(),
        };
        match repo.commit(self.branch, self.tip, &commit) {
            Ok(id) => Ok(Saved::Committed {
                sha,
                commit: id.to_hex(),
                merged,
            }),
            // Only a writer outside the lock moves the branch here.
            Err(Error::Conflict { .. } | Error::RefClash { .. }) => Ok(Saved::Moved {
                message: format!(
                    "{} moved while {} was being committed",
                    self.branch, self.path
                ),
            }),
            Err(e) => Err(said(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_message_is_the_diff_of_the_file_kind_the_path_is() {
        let deck = "name = \"Burn\"\ncards = []\n";
        let renamed = "name = \"Burn!\"\ncards = []\n";
        assert_eq!(
            message_for("burn.deck.toml", deck, renamed).unwrap(),
            "burn: name: \"Burn\" → \"Burn!\""
        );
        assert!(
            message_for("collection.toml", "", "cards = []\n\n[places]\nBulk = {}\n")
                .unwrap()
                .starts_with("collection: ")
        );
        assert!(message_for(
            "meldweb.toml",
            "",
            "[printings]\nrank = [{ avoid = \"is:digital\" }]\n"
        )
        .unwrap()
        .starts_with("meldweb.toml: "));
        assert_eq!(
            message_for("lantern.criteria.toml", "", "x").unwrap(),
            "lantern: criteria made"
        );
        assert_eq!(
            message_for("cards/a.card.toml", "", "name = 1").unwrap(),
            "a: custom card made"
        );
        assert_eq!(
            message_for("cards/a.card.toml", "x", "y").unwrap(),
            "a: custom card edited"
        );
    }

    #[test]
    fn a_path_curator_does_not_edit_has_no_message() {
        for path in [
            "VERSION",
            "decks/notes.md",
            "cards/a/b.card.toml",
            "decks/x.criteria.toml",
        ] {
            assert_eq!(message_for(path, "", "x"), Err(Unsaid::NotEdited), "{path}");
        }
    }

    #[test]
    fn a_text_that_is_no_longer_a_deck_is_unreadable() {
        let deck = "name = \"Lantern\"\ncards = []\n";
        assert!(message_for("lantern.deck.toml", deck, deck).is_ok());
        assert!(matches!(
            message_for("lantern.deck.toml", deck, "name = \"Lantern\"\ncards = [\n"),
            Err(Unsaid::Unreadable(_))
        ));
    }
}
