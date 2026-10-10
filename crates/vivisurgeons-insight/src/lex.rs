//! Oracle text to tokens. Reminder text is dropped here, because it restates a
//! keyword the parser already reads, and the card's own name becomes `SelfRef`
//! so that "Llanowar Elves" and "this creature" are the same thing downstream.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    /// Lowercased.
    Word(String),
    Num(u32),
    /// The inside of one `{…}`, as printed: `2`, `G`, `T`, `W/U`, `X`.
    Sym(String),
    /// `+1`, `−2`, `+X` on its own: loyalty costs and the odd `+N` bonus.
    Signed(Signed),
    /// `+1/+1`, `-2/-0`, `2/2`, `*/*`, `X/X`.
    Pt(Pt),
    SelfRef,
    Comma,
    Period,
    Colon,
    Semi,
    Dash,
    Bullet,
    Quote,
    /// A possessive `'s` or a plural possessive's trailing `'`.
    Possessive,
    Other(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signed {
    N(i32),
    PlusX,
    MinusX,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtPart {
    N(i32),
    X,
    MinusX,
    Star,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pt {
    pub power: PtPart,
    pub toughness: PtPart,
    /// `+1/+1` modifies; `1/1` is a size.
    pub signed: bool,
}

/// Private use, so no printed card can contain it.
const SELF: char = '\u{E000}';

/// Contractions that would otherwise split into a word and a possessive.
const CONTRACTIONS: &[&str] = &["that's", "it's", "there's", "who's", "what's", "let's"];

/// Replaces every way a card names itself with the self marker. A legendary
/// card is also called by the part of its name before the comma ("Emry" for
/// "Emry, Lurker of the Loch").
pub fn mark_self(text: &str, names: &[&str], legendary: bool) -> String {
    let mut out = text.to_string();
    let mut all: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    if legendary {
        for n in names {
            if let Some((short, _)) = n.split_once(',') {
                all.push(short.to_string());
            }
        }
    }
    // Longest first, so the full name is not eaten by its short form.
    all.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for n in all.iter().filter(|n| !n.is_empty()) {
        out = out.replace(n.as_str(), &SELF.to_string());
    }
    out
}

pub fn lex(text: &str) -> Vec<Tok> {
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        match c {
            _ if c.is_whitespace() => i += 1,
            '(' => {
                let mut depth = 0;
                while i < cs.len() {
                    match cs[i] {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            '{' => {
                let start = i + 1;
                while i < cs.len() && cs[i] != '}' {
                    i += 1;
                }
                out.push(Tok::Sym(cs[start..i].iter().collect()));
                i += 1;
            }
            SELF => {
                out.push(Tok::SelfRef);
                i += 1;
            }
            // "~'s": a possessive with no word in front for push_word to split.
            '\'' | '’'
                if cs.get(i + 1) == Some(&'s')
                    && cs.get(i + 2).is_none_or(|c| !c.is_alphanumeric()) =>
            {
                out.push(Tok::Possessive);
                i += 2;
            }
            '"' | '“' | '”' => {
                out.push(Tok::Quote);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '.' => {
                out.push(Tok::Period);
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            ';' => {
                out.push(Tok::Semi);
                i += 1;
            }
            '—' | '–' => {
                out.push(Tok::Dash);
                i += 1;
            }
            '•' => {
                out.push(Tok::Bullet);
                i += 1;
            }
            _ => {
                if let Some((tok, next)) = numeric(&cs, i) {
                    out.push(tok);
                    i = next;
                } else if c.is_alphabetic() {
                    let start = i;
                    while i < cs.len()
                        && (cs[i].is_alphanumeric() || matches!(cs[i], '\'' | '’' | '-'))
                    {
                        i += 1;
                    }
                    let raw: String = cs[start..i]
                        .iter()
                        .map(|c| if *c == '’' { '\'' } else { *c })
                        .collect::<String>()
                        .to_lowercase();
                    push_word(&mut out, raw);
                } else {
                    out.push(Tok::Other(c));
                    i += 1;
                }
            }
        }
    }
    out
}

fn push_word(out: &mut Vec<Tok>, raw: String) {
    if CONTRACTIONS.contains(&raw.as_str()) {
        out.push(Tok::Word(raw));
    } else if let Some(stem) = raw.strip_suffix("'s") {
        out.push(Tok::Word(stem.to_string()));
        out.push(Tok::Possessive);
    } else if let Some(stem) = raw.strip_suffix('\'') {
        out.push(Tok::Word(stem.to_string()));
        out.push(Tok::Possessive);
    } else {
        out.push(Tok::Word(raw));
    }
}

fn is_minus(c: char) -> bool {
    c == '-' || c == '−'
}

/// One P/T part at `i`: optional sign, then digits, `X` or `*`.
fn pt_part(cs: &[char], mut i: usize) -> Option<(PtPart, bool, usize)> {
    let mut neg = false;
    let mut signed = false;
    if i < cs.len() && (cs[i] == '+' || is_minus(cs[i])) {
        neg = is_minus(cs[i]);
        signed = true;
        i += 1;
    }
    let c = *cs.get(i)?;
    if c == 'X' {
        let part = if neg { PtPart::MinusX } else { PtPart::X };
        return Some((part, signed, i + 1));
    }
    if c == '*' {
        return Some((PtPart::Star, signed, i + 1));
    }
    let start = i;
    while i < cs.len() && cs[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    let n: i32 = cs[start..i].iter().collect::<String>().parse().ok()?;
    Some((PtPart::N(if neg { -n } else { n }), signed, i))
}

fn numeric(cs: &[char], i: usize) -> Option<(Tok, usize)> {
    let c = cs[i];
    let starts = c.is_ascii_digit() || c == '+' || is_minus(c) || c == '*' || c == 'X';
    if !starts {
        return None;
    }
    let (power, signed, j) = pt_part(cs, i)?;
    if cs.get(j) == Some(&'/') {
        if let Some((toughness, _, k)) = pt_part(cs, j + 1) {
            // "X/X" must not swallow "X" the word followed by something else.
            if k >= cs.len() || !cs[k].is_alphanumeric() {
                return Some((
                    Tok::Pt(Pt {
                        power,
                        toughness,
                        signed,
                    }),
                    k,
                ));
            }
        }
    }
    // A lone X or * is a word or punctuation, not a number.
    if j < cs.len() && cs[j].is_alphanumeric() {
        return None;
    }
    match (power, signed) {
        (PtPart::N(n), true) => Some((Tok::Signed(Signed::N(n)), j)),
        (PtPart::X, true) => Some((Tok::Signed(Signed::PlusX), j)),
        (PtPart::MinusX, true) => Some((Tok::Signed(Signed::MinusX), j)),
        (PtPart::N(n), false) => Some((Tok::Num(n as u32), j)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_text_is_dropped_and_the_name_is_self() {
        let text = mark_self(
            "Flying (This creature can't be blocked.)\nWhen Emry enters, mill four cards.",
            &["Emry, Lurker of the Loch"],
            true,
        );
        let toks = lex(&text);
        assert!(toks.contains(&Tok::SelfRef));
        assert!(!toks.contains(&Tok::Word("blocked".into())));
    }

    #[test]
    fn modifiers_sizes_and_loyalty() {
        assert_eq!(
            lex("+1/+1")[0],
            Tok::Pt(Pt {
                power: PtPart::N(1),
                toughness: PtPart::N(1),
                signed: true
            })
        );
        assert_eq!(
            lex("−3:")[0],
            Tok::Signed(Signed::N(-3)),
            "Scryfall writes loyalty minus as U+2212"
        );
        assert_eq!(lex("X/X")[0].clone(), {
            Tok::Pt(Pt {
                power: PtPart::X,
                toughness: PtPart::X,
                signed: false,
            })
        });
        assert_eq!(lex("owner's")[1], Tok::Possessive);
        assert_eq!(lex("that's")[0], Tok::Word("that's".into()));
    }
}
