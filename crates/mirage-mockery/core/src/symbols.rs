//! The mana and tap symbols, and rules text split into the runs of words and
//! symbols the template sets.
//!
//! The symbols are Scryfall's SVGs, committed in `symbols/` and compiled in by
//! `build.rs`, so setting one needs no disk or network, in a binary or in the
//! browser. `symbols/fetch.sh` refreshes them.

static SVGS: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/symbols.rs"));

/// A symbol's SVG, by what goes between its braces: `U`, `U/P`, `T`.
pub fn svg(code: &str) -> Option<&'static [u8]> {
    SVGS.iter().find(|(c, _)| *c == code).map(|(_, svg)| *svg)
}

/// A symbol's file name: `U/P` is `U_P.svg`, because a slash is a directory.
pub fn file_name(code: &str) -> String {
    format!("{}.svg", code.replace('/', "_"))
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

    #[test]
    fn every_symbol_cards_print_is_compiled_in() {
        // CR 107.4's mana symbols, a sample of each kind, and the tap symbol.
        for code in [
            "W", "U", "B", "R", "G", "C", "0", "15", "X", "W/U", "2/W", "C/G", "U/P", "G/U/P", "S",
            "T", "Q", "E",
        ] {
            assert!(svg(code).is_some(), "{{{code}}}");
        }
    }

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
