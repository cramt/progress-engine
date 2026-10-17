//! The mana and tap symbols, as SVGs in a directory, and rules text split
//! into the runs of words and symbols the template sets.
//!
//! The symbols are Wizards' (the Fan Content Policy lists them among its
//! marks), so the repo carries none. `mockery symbols` fetches Scryfall's into
//! a cache once; tests use the stand-ins in `tests/symbols/`. Nothing here
//! touches the network, so the library builds for the browser.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Symbols by what goes between the braces: `U`, `U/P`, `T`.
pub struct Symbols {
    pub svgs: BTreeMap<String, Vec<u8>>,
}

/// A symbol's file name: `U/P` is `U_P.svg`, because a slash is a directory.
pub fn file_name(code: &str) -> String {
    format!("{}.svg", code.replace('/', "_"))
}

impl Symbols {
    pub fn default_dir() -> PathBuf {
        let base = std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache")
            });
        base.join("mirage-mockery").join("symbols")
    }

    pub fn open(dir: &Path) -> Result<Self> {
        let entries = std::fs::read_dir(dir).with_context(|| {
            format!(
                "no symbols at {}; `mockery symbols` fetches them",
                dir.display()
            )
        })?;
        let mut svgs = BTreeMap::new();
        for entry in entries {
            let path = entry?.path();
            let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".svg"))
            else {
                continue;
            };
            svgs.insert(stem.replace('_', "/"), std::fs::read(&path)?);
        }
        if svgs.is_empty() {
            bail!(
                "{} holds no symbols; `mockery symbols` fetches them",
                dir.display()
            );
        }
        Ok(Symbols { svgs })
    }

    pub fn require(&self, code: &str) -> Result<()> {
        if self.svgs.contains_key(code) {
            Ok(())
        } else {
            bail!("no symbol {{{code}}}")
        }
    }
}

/// One stretch of a paragraph, set as one thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Words { text: String, reminder: bool },
    Symbol(String),
}

/// A paragraph of rules text as runs: symbols out of their braces, and
/// reminder text (anything in parentheses) marked so it is set in italics.
pub fn runs(paragraph: &str) -> Vec<Run> {
    let mut out = Vec::new();
    let mut words = String::new();
    let mut depth = 0usize;
    let mut chars = paragraph.chars().peekable();
    let flush = |words: &mut String, out: &mut Vec<Run>, reminder: bool| {
        if !words.is_empty() {
            out.push(Run::Words {
                text: std::mem::take(words),
                reminder,
            });
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let code: String = chars.by_ref().take_while(|&c| c != '}').collect();
                flush(&mut words, &mut out, depth > 0);
                out.push(Run::Symbol(code));
            }
            '(' => {
                flush(&mut words, &mut out, depth > 0);
                depth += 1;
                words.push(c);
            }
            ')' => {
                words.push(c);
                flush(&mut words, &mut out, true);
                depth = depth.saturating_sub(1);
            }
            _ => words.push(c),
        }
    }
    flush(&mut words, &mut out, depth > 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str, reminder: bool) -> Run {
        Run::Words {
            text: text.into(),
            reminder,
        }
    }

    #[test]
    fn symbols_leave_their_braces_and_reminder_text_is_marked() {
        assert_eq!(
            runs("{1}{U}, {T}: Scry 1. (Look at the top card of your library.)"),
            [
                Run::Symbol("1".into()),
                Run::Symbol("U".into()),
                words(", ", false),
                Run::Symbol("T".into()),
                words(": Scry 1. ", false),
                words("(Look at the top card of your library.)", true),
            ]
        );
    }

    #[test]
    fn a_symbol_inside_reminder_text_keeps_the_italics_around_it() {
        assert_eq!(
            runs("(Pay {U} or 2 life.)"),
            [
                words("(Pay ", true),
                Run::Symbol("U".into()),
                words(" or 2 life.)", true),
            ]
        );
    }
}
