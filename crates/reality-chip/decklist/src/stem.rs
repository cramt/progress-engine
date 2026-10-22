//! A deck's stem: what names a deck in the Magic repo (ADR-0041). The deck is
//! the branch `decks/<stem>/main`, its file `<stem>.deck.toml`, and a
//! collection's deck place names it by the stem alone.
//!
//! The stem is fixed when the deck is made and never follows a rename, as the
//! path was before it (ADR-0021). It is one component of a branch name and of
//! a file name, so it holds only what both take everywhere.

use std::fmt;

use thiserror::Error;

const SUFFIX: &str = ".deck.toml";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeckStem(String);

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{0:?} is not a deck's stem: one name of ASCII letters, digits, '.', '_' and '-', not starting with '.' or '-', like \"lantern\"")]
pub struct BadStem(pub String);

impl DeckStem {
    pub fn new(stem: impl Into<String>) -> Result<Self, BadStem> {
        let stem = stem.into();
        let ok = !stem.is_empty()
            && stem
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            && !stem.starts_with(['.', '-'])
            && !stem.ends_with('.')
            && !stem.ends_with(".lock")
            && !stem.contains("..");
        if ok {
            Ok(Self(stem))
        } else {
            Err(BadStem(stem))
        }
    }

    /// The stem of a repo-version-1 deck path, `decks/<stem>.deck.toml`, or
    /// `None` for anything else.
    pub fn from_v1_path(path: &str) -> Option<Self> {
        let stem = path.strip_prefix("decks/")?.strip_suffix(SUFFIX)?;
        Self::new(stem).ok()
    }

    /// A stem as written, or the stem of a repo-version-1 path: what reading
    /// another player's collection, or an old file, has to accept.
    pub fn read(text: &str) -> Result<Self, BadStem> {
        Self::from_v1_path(text).map_or_else(|| Self::new(text), Ok)
    }

    /// The stem of a deck file's name, `<anything>/<stem>.deck.toml`.
    pub fn of_file(path: &str) -> Option<Self> {
        let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
        Self::new(file.strip_suffix(SUFFIX)?).ok()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `<stem>.deck.toml`: the deck's file in its own tree.
    pub fn deck_file(&self) -> String {
        format!("{}{SUFFIX}", self.0)
    }

    /// `<stem>.criteria.toml`: its Gauntlet criteria, beside it.
    pub fn criteria_file(&self) -> String {
        format!("{}.criteria.toml", self.0)
    }

    /// `decks/<stem>/<branch>`: one of the deck's branches.
    pub fn branch(&self, branch: &str) -> String {
        format!("decks/{}/{branch}", self.0)
    }
}

impl fmt::Display for DeckStem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stem_is_one_safe_name_and_a_v1_path_reads_as_one() {
        for good in ["lantern", "budget-lantern", "a.b_c", "x1"] {
            assert!(DeckStem::new(good).is_ok(), "{good}");
        }
        for bad in ["", "a/b", ".x", "-x", "x.", "x.lock", "a..b", "a b", "ä"] {
            assert!(DeckStem::new(bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            DeckStem::read("decks/lantern.deck.toml").unwrap().as_str(),
            "lantern"
        );
        assert_eq!(DeckStem::read("lantern").unwrap().as_str(), "lantern");
        assert!(DeckStem::read("decks/a/b.deck.toml").is_err());
        assert_eq!(
            DeckStem::of_file("/tmp/mtg/loam.deck.toml")
                .unwrap()
                .as_str(),
            "loam"
        );
        let s = DeckStem::new("loam").unwrap();
        assert_eq!(s.branch("main"), "decks/loam/main");
        assert_eq!(s.deck_file(), "loam.deck.toml");
    }
}
