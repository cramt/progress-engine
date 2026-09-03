//! Lexer and recursive-descent parser for the supported Scryfall syntax subset.

use thiserror::Error;

use crate::{Cmp, Colors, IsProperty, Query};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("empty query")]
    Empty,
    #[error(
        "unknown search key {key:?} in term {term:?} (supported: t, o, name, kw, cat, mv, cmc, id, is)"
    )]
    UnknownKey { key: String, term: String },
    #[error("unknown is: property {value:?} (supported: permanent, spell, historic)")]
    UnknownIsProperty { value: String },
    #[error(
        "{term:?}: {key}: asks whether a card has a value, not how it compares. \
         Write {key}:value, or -{key}:value for the cards without it"
    )]
    NoComparison { key: String, term: String },
    #[error("{term:?}: {value:?} is not a number")]
    BadNumber { term: String, value: String },
    #[error("{term:?}: {value:?} is not a colour identity (use letters from wubrg, or c)")]
    BadColors { term: String, value: String },
    #[error("{term:?}: missing a value after the operator")]
    MissingValue { term: String },
    #[error("unbalanced parenthesis")]
    UnbalancedParen,
    #[error("unexpected {0:?}")]
    Unexpected(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    LParen,
    RParen,
    Or,
    Minus,
    Term(String),
}

fn lex(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut out = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match c {
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            '-' => {
                out.push(Token::Minus);
                i += 1;
            }
            _ => {
                let mut term = String::new();
                let mut was_quoted = false;
                while i < chars.len() {
                    let c = chars[i];
                    if c == '"' {
                        was_quoted = true;
                        i += 1;
                        while i < chars.len() && chars[i] != '"' {
                            term.push(chars[i]);
                            i += 1;
                        }
                        i += 1; // closing quote (tolerated if absent)
                        continue;
                    }
                    if c.is_whitespace() || c == '(' || c == ')' {
                        break;
                    }
                    term.push(c);
                    i += 1;
                }
                // A quoted token is always a value, never the `or` keyword.
                if !was_quoted && term.eq_ignore_ascii_case("or") {
                    out.push(Token::Or);
                } else if !was_quoted && term.eq_ignore_ascii_case("and") {
                    // Explicit `and` is a no-op; juxtaposition already means AND.
                } else {
                    out.push(Token::Term(term));
                }
            }
        }
    }
    Ok(out)
}

/// Split `key<op>value`, returning None when there is no operator (a bare word).
fn split_term(term: &str) -> Option<(&str, Cmp, &str)> {
    let bytes = term.as_bytes();
    let pos = bytes
        .iter()
        .position(|b| matches!(b, b':' | b'=' | b'<' | b'>' | b'!'))?;
    let key = &term[..pos];
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest = &term[pos..];
    let (cmp, vstart) = match rest.as_bytes() {
        [b'<', b'=', ..] => (Cmp::Le, 2),
        [b'>', b'=', ..] => (Cmp::Ge, 2),
        [b'!', b'=', ..] => (Cmp::Ne, 2),
        [b'<', ..] => (Cmp::Lt, 1),
        [b'>', ..] => (Cmp::Gt, 1),
        [b'=', ..] => (Cmp::Eq, 1),
        [b':', ..] => (Cmp::Eq, 1), // refined per-key below
        _ => return None,
    };
    Some((key, cmp, &rest[vstart..]))
}

fn parse_term(term: &str) -> Result<Query, ParseError> {
    let Some((key, cmp, value)) = split_term(term) else {
        // No operator: a bare word is a name substring.
        return Ok(Query::Name(term.to_string()));
    };
    if value.is_empty() {
        return Err(ParseError::MissingValue {
            term: term.to_string(),
        });
    }
    let colon = term[key.len()..].starts_with(':');

    match key.to_ascii_lowercase().as_str() {
        "t" | "type" => Ok(Query::Type(value.to_string())),
        "o" | "oracle" => Ok(Query::Oracle(value.to_string())),
        "name" => Ok(Query::Name(value.to_string())),
        "kw" | "keyword" => {
            // A keyword is had or not had, so `kw:` and `kw=` are the only
            // forms that mean anything. Scryfall answers `kw>=flying` with
            // "didn't match any cards" — the silent no-match this crate
            // refuses, so this is the one place `kw:` deliberately differs
            // from Scryfall, and it differs by being louder.
            if cmp != Cmp::Eq {
                return Err(ParseError::NoComparison {
                    key: key.to_ascii_lowercase(),
                    term: term.to_string(),
                });
            }
            Ok(Query::Keyword(value.to_string()))
        }
        "cat" | "category" => Ok(Query::Category(value.to_string())),
        "mv" | "cmc" => {
            let n: f64 = value.parse().map_err(|_| ParseError::BadNumber {
                term: term.to_string(),
                value: value.to_string(),
            })?;
            Ok(Query::ManaValue(cmp, n))
        }
        "id" | "identity" => {
            let colors = Colors::from_letters(value).ok_or_else(|| ParseError::BadColors {
                term: term.to_string(),
                value: value.to_string(),
            })?;
            // Scryfall treats `id:` as "fits inside", i.e. `<=`.
            let cmp = if colon { Cmp::Le } else { cmp };
            Ok(Query::Identity(cmp, colors))
        }
        "is" => match value.to_ascii_lowercase().as_str() {
            "permanent" => Ok(Query::Is(IsProperty::Permanent)),
            "spell" => Ok(Query::Is(IsProperty::Spell)),
            "historic" => Ok(Query::Is(IsProperty::Historic)),
            other => Err(ParseError::UnknownIsProperty {
                value: other.to_string(),
            }),
        },
        other => Err(ParseError::UnknownKey {
            key: other.to_string(),
            term: term.to_string(),
        }),
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn or_expr(&mut self) -> Result<Query, ParseError> {
        let mut parts = vec![self.and_expr()?];
        while matches!(self.peek(), Some(Token::Or)) {
            self.pos += 1;
            parts.push(self.and_expr()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().expect("length checked")
        } else {
            Query::Or(parts)
        })
    }

    fn and_expr(&mut self) -> Result<Query, ParseError> {
        let mut parts = vec![self.unary()?];
        while matches!(
            self.peek(),
            Some(Token::Term(_) | Token::Minus | Token::LParen)
        ) {
            parts.push(self.unary()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().expect("length checked")
        } else {
            Query::And(parts)
        })
    }

    fn unary(&mut self) -> Result<Query, ParseError> {
        if matches!(self.peek(), Some(Token::Minus)) {
            self.pos += 1;
            return Ok(Query::Not(Box::new(self.unary()?)));
        }
        match self.tokens.get(self.pos).cloned() {
            Some(Token::LParen) => {
                self.pos += 1;
                let inner = self.or_expr()?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.pos += 1;
                        Ok(inner)
                    }
                    _ => Err(ParseError::UnbalancedParen),
                }
            }
            Some(Token::Term(t)) => {
                self.pos += 1;
                parse_term(&t)
            }
            Some(Token::RParen) => Err(ParseError::UnbalancedParen),
            Some(Token::Or) => Err(ParseError::Unexpected("or".into())),
            Some(Token::Minus) => unreachable!("handled above"),
            None => Err(ParseError::Empty),
        }
    }
}

/// Parse a Scryfall-syntax query.
pub fn parse(input: &str) -> Result<Query, ParseError> {
    let tokens = lex(input)?;
    if tokens.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut p = Parser { tokens, pos: 0 };
    let q = p.or_expr()?;
    match p.peek() {
        None => Ok(q),
        Some(Token::RParen) => Err(ParseError::UnbalancedParen),
        Some(t) => Err(ParseError::Unexpected(format!("{t:?}"))),
    }
}
