//! Lexer and recursive-descent parser for the supported Scryfall syntax subset.
//!
//! Every error here names the term that caused it and, where the accepted
//! values are a closed set, lists them. That is the whole discipline: an
//! unsupported term must cost you a message, not a silent zero.

use thiserror::Error;

use crate::legality::FORMATS;
use crate::{
    Cmp, ColorField, ColorSpec, Colors, FormatStatus, IsProperty, Query, Rarity, Stat, StatOperand,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("empty query")]
    Empty,
    #[error(
        "unknown search key {key:?} in term {term:?} (supported: {})",
        supported_keys()
    )]
    UnknownKey { key: String, term: String },
    #[error("unknown is: property {value:?} (supported: {})", is_properties())]
    UnknownIsProperty { value: String },
    #[error(
        "{term:?}: {key}: asks whether a card has a value, not how it compares. \
         Write {key}:value, or -{key}:value for the cards without it"
    )]
    NoComparison { key: String, term: String },
    #[error("{term:?}: {value:?} is not a number")]
    BadNumber { term: String, value: String },
    #[error(
        "{term:?}: {value:?} is not a colour (use letters from wubrg, a colour name, \
         a guild/shard/wedge nickname, c for colourless, m for multicolour, or a number)"
    )]
    BadColors { term: String, value: String },
    #[error("{term:?}: {value:?} is not a rarity (supported: common, uncommon, rare, special, mythic, bonus)")]
    BadRarity { term: String, value: String },
    #[error("{term:?}: {value:?} is not a format (supported: {})", FORMATS.join(", "))]
    BadFormat { term: String, value: String },
    #[error("{term:?}: {value:?} is neither a number nor a statistic to compare against (try pow, tou, pt, loy or def)")]
    BadStatOperand { term: String, value: String },
    #[error(
        "{term:?}: {value:?} is not a mana cost (write symbols in braces, \
         or unbraced shorthand for the simple ones: 2WW)"
    )]
    BadManaCost { term: String, value: String },
    #[error(
        "{term:?}: a devotion term asks about one set of colours at a time, and \
         {value:?} names more than one. Write devotion:{{u}}{{u}} for blue, or \
         devotion:{{u/b}}{{u/b}} for blue and black together"
    )]
    MixedDevotion { term: String, value: String },
    #[error("{term:?}: missing a value after the operator")]
    MissingValue { term: String },
    #[error("unbalanced parenthesis")]
    UnbalancedParen,
    #[error("unexpected {0:?}")]
    Unexpected(String),
}

/// Every key a term may start with, one row per key and its other spellings.
///
/// **The gate, not a description of it.** A key is checked against this table
/// before anything else reads it, so a key the parser accepts is a key this
/// table lists, and the `supported:` list in [`ParseError::UnknownKey`] is this
/// table printed. It used to be a separate hand-kept list that fell seventeen
/// keys behind the match it described, so a mistyped term was told `rarity:`,
/// `set:` and `format:` did not exist (#46). A key added to the match and not
/// here is refused as unknown, which the first test of it notices; a key here
/// with no arm is caught by the test that parses every one of them.
pub const KEYS: &[&[&str]] = &[
    &["t", "type"],
    &["o", "oracle"],
    &["fo", "fulloracle"],
    &["name"],
    &["kw", "keyword"],
    &["otag", "oracletag"],
    &["cat", "category"],
    &["mv", "cmc", "manavalue"],
    &["c", "color", "colors", "colour", "colours"],
    &["id", "identity"],
    &["produces", "prod", "produced"],
    &["pow", "power"],
    &["tou", "toughness"],
    &["pt", "powtou"],
    &["loy", "loyalty"],
    &["def", "defense", "defence"],
    &["r", "rarity"],
    &["s", "e", "set", "edition"],
    &["f", "format"],
    &["banned"],
    &["restricted"],
    &["m", "mana"],
    &["devotion", "dev"],
    &["layout"],
    &["is", "not"],
];

fn supported_keys() -> String {
    KEYS.iter()
        .map(|row| match row {
            [key] => key.to_string(),
            [key, rest @ ..] => format!("{key} ({})", rest.join(", ")),
            [] => String::new(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_properties() -> String {
    IS_PROPERTIES
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
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

/// Colour nicknames, as Scryfall accepts them.
///
/// Data rather than code because that is what they are — a naming convention
/// with no rule behind it. Getting one wrong (Abzan is WBG, not WBR) is a query
/// that runs perfectly and answers about the wrong deck, so they are written
/// out where they can be read against a colour pie.
const COLOR_NICKNAMES: [(&str, &str); 41] = [
    // Single colours, spelled out.
    ("white", "w"),
    ("blue", "u"),
    ("black", "b"),
    ("red", "r"),
    ("green", "g"),
    // Guilds.
    ("azorius", "wu"),
    ("dimir", "ub"),
    ("rakdos", "br"),
    ("gruul", "rg"),
    ("selesnya", "gw"),
    ("orzhov", "wb"),
    ("izzet", "ur"),
    ("golgari", "bg"),
    ("boros", "rw"),
    ("simic", "gu"),
    // Strixhaven colleges, which are the guild pairs under other names.
    ("silverquill", "wb"),
    ("prismari", "ur"),
    ("witherbloom", "bg"),
    ("lorehold", "rw"),
    ("quandrix", "gu"),
    // Shards.
    ("bant", "gwu"),
    ("esper", "wub"),
    ("grixis", "ubr"),
    ("jund", "brg"),
    ("naya", "rgw"),
    // Wedges.
    ("abzan", "wbg"),
    ("jeskai", "urw"),
    ("sultai", "bgu"),
    ("mardu", "rwb"),
    ("temur", "gur"),
    // Four-colour names, each the colour it is missing.
    ("artifice", "wubr"),
    ("chaos", "ubrg"),
    ("aggression", "brgw"),
    ("altruism", "rgwu"),
    ("growth", "gwub"),
    // Five.
    ("wubrg", "wubrg"),
    ("rainbow", "wubrg"),
    // Guild-adjacent spellings Scryfall also takes.
    ("colorless", "c"),
    ("colourless", "c"),
    ("multicolor", "m"),
    ("multicolour", "m"),
];

/// Read the right-hand side of a colour term.
///
/// `c` is the one value whose meaning depends on the key. For a card's colours
/// or identity it means *no colour*, which is the absence of a symbol; for
/// `produces:` it means the `{C}` symbol, which is a thing a Sol Ring makes.
/// Collapsing the two would make `produces:c` either trivially true or
/// impossible, depending which way you collapsed it.
fn parse_color_spec(field: ColorField, value: &str) -> Option<ColorSpec> {
    let lower = value.to_ascii_lowercase();
    if let Ok(n) = lower.parse::<u32>() {
        return Some(ColorSpec::Count(n));
    }
    let letters = COLOR_NICKNAMES
        .iter()
        .find(|(name, _)| *name == lower)
        .map_or(lower.as_str(), |(_, letters)| letters);

    if letters == "m" {
        return Some(ColorSpec::Multicolor);
    }
    if letters == "c" {
        return Some(match field {
            ColorField::Produces => ColorSpec::Set(Colors::from_mana_letters("c")?),
            _ => ColorSpec::Colorless,
        });
    }
    let colors = match field {
        ColorField::Produces => Colors::from_mana_letters(letters)?,
        _ => Colors::from_letters(letters)?,
    };
    Some(ColorSpec::Set(colors))
}

/// The `is:` properties, paired with the words that select them.
const IS_PROPERTIES: [(&str, IsProperty); 24] = [
    ("permanent", IsProperty::Permanent),
    ("spell", IsProperty::Spell),
    ("historic", IsProperty::Historic),
    ("vanilla", IsProperty::Vanilla),
    ("bear", IsProperty::Bear),
    ("dfc", IsProperty::DoubleFaced),
    ("doublefaced", IsProperty::DoubleFaced),
    ("mdfc", IsProperty::ModalDoubleFaced),
    ("transform", IsProperty::Transform),
    ("tdfc", IsProperty::Transform),
    ("split", IsProperty::Split),
    ("flip", IsProperty::Flip),
    ("meld", IsProperty::Meld),
    ("leveler", IsProperty::Leveler),
    ("adventure", IsProperty::Adventure),
    ("hybrid", IsProperty::Hybrid),
    ("phyrexian", IsProperty::Phyrexian),
    ("commander", IsProperty::Commander),
    ("partner", IsProperty::Partner),
    ("companion", IsProperty::Companion),
    ("reserved", IsProperty::Reserved),
    ("gamechanger", IsProperty::GameChanger),
    ("game_changer", IsProperty::GameChanger),
    ("modal_dfc", IsProperty::ModalDoubleFaced),
];

/// The statistics that compare numerically, and the words for them.
const STATS: [(&str, Stat); 11] = [
    ("pow", Stat::Power),
    ("power", Stat::Power),
    ("tou", Stat::Toughness),
    ("toughness", Stat::Toughness),
    ("pt", Stat::PowerPlusToughness),
    ("powtou", Stat::PowerPlusToughness),
    ("loy", Stat::Loyalty),
    ("loyalty", Stat::Loyalty),
    ("def", Stat::Defense),
    ("defense", Stat::Defense),
    ("defence", Stat::Defense),
];

fn stat_named(word: &str) -> Option<Stat> {
    STATS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|&(_, s)| s)
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
    let key = key.to_ascii_lowercase();
    if !KEYS.iter().any(|row| row.contains(&key.as_str())) {
        return Err(ParseError::UnknownKey {
            key,
            term: term.to_string(),
        });
    }

    // Keys that ask whether a card *has* a value rather than how it compares.
    // Scryfall answers `kw>=flying` with "didn't match any cards" — the silent
    // no-match this crate refuses — so these are the places it deliberately
    // differs from Scryfall, and it differs by being louder.
    let require_equality = |q: Query| -> Result<Query, ParseError> {
        if cmp != Cmp::Eq {
            return Err(ParseError::NoComparison {
                key: key.clone(),
                term: term.to_string(),
            });
        }
        Ok(q)
    };

    if let Some(stat) = stat_named(&key) {
        let operand = match stat_named(&value.to_ascii_lowercase()) {
            Some(other) => StatOperand::Stat(other),
            None => StatOperand::Number(value.parse::<f64>().map_err(|_| {
                ParseError::BadStatOperand {
                    term: term.to_string(),
                    value: value.to_string(),
                }
            })?),
        };
        return Ok(Query::Stat(stat, cmp, operand));
    }

    let color_field = match key.as_str() {
        "c" | "color" | "colors" | "colour" | "colours" => Some(ColorField::Color),
        "id" | "identity" => Some(ColorField::Identity),
        "produces" | "prod" | "produced" => Some(ColorField::Produces),
        _ => None,
    };
    if let Some(field) = color_field {
        let spec = parse_color_spec(field, value).ok_or_else(|| ParseError::BadColors {
            term: term.to_string(),
            value: value.to_string(),
        })?;
        // A colon means "fits inside" for identity and "contains all of" for
        // the other two; see `ColorField::colon_means`. A count compares as a
        // number whatever the key, because `c:2` reads as "exactly two".
        let cmp = match (colon, spec) {
            (true, ColorSpec::Count(_)) => Cmp::Eq,
            (true, _) => field.colon_means(),
            (false, _) => cmp,
        };
        return Ok(Query::Colors(field, cmp, spec));
    }

    match key.as_str() {
        "t" | "type" => Ok(Query::Type(value.to_string())),
        "o" | "oracle" => Ok(Query::Oracle(value.to_string())),
        "fo" | "fulloracle" => Ok(Query::FullOracle(value.to_string())),
        "name" => Ok(Query::Name(value.to_string())),
        "kw" | "keyword" => require_equality(Query::Keyword(value.to_string())),
        "otag" | "oracletag" => require_equality(Query::Tag(value.to_string())),
        "cat" | "category" => require_equality(Query::Category(value.to_string())),
        "mv" | "cmc" | "manavalue" => match value.to_ascii_lowercase().as_str() {
            "even" => Ok(Query::ManaValueParity { even: true }),
            "odd" => Ok(Query::ManaValueParity { even: false }),
            _ => {
                let n: f64 = value.parse().map_err(|_| ParseError::BadNumber {
                    term: term.to_string(),
                    value: value.to_string(),
                })?;
                Ok(Query::ManaValue(cmp, n))
            }
        },
        "r" | "rarity" => {
            let rarity = Rarity::parse(value).ok_or_else(|| ParseError::BadRarity {
                term: term.to_string(),
                value: value.to_string(),
            })?;
            Ok(Query::Rarity(cmp, rarity))
        }
        "s" | "e" | "set" | "edition" => require_equality(Query::Set(value.to_string())),
        "f" | "format" => format_term(term, value, FormatStatus::Legal, cmp, &key),
        "banned" => format_term(term, value, FormatStatus::Banned, cmp, &key),
        "restricted" => format_term(term, value, FormatStatus::Restricted, cmp, &key),
        "m" | "mana" => {
            let cost = crate::mana::ManaCost::parse(value);
            if cost.is_empty() {
                return Err(ParseError::BadManaCost {
                    term: term.to_string(),
                    value: value.to_string(),
                });
            }
            // A colon is "contains at least", as it is on Scryfall: `m:{G}`
            // finds every green card rather than only the one that costs {G}.
            Ok(Query::Mana(if colon { Cmp::Ge } else { cmp }, cost))
        }
        "devotion" | "dev" => {
            let spec = crate::mana::ManaCost::parse(value);
            let level = spec.symbol_count();
            if level == 0 {
                return Err(ParseError::BadManaCost {
                    term: term.to_string(),
                    value: value.to_string(),
                });
            }
            // Devotion is counted towards a colour or a pair, so every symbol
            // in the term has to name the same one. `devotion:{u}{b}` is two
            // questions, and answering either of them would be a guess.
            if !spec.symbols_agree_on_colors() {
                return Err(ParseError::MixedDevotion {
                    term: term.to_string(),
                    value: value.to_string(),
                });
            }
            Ok(Query::Devotion(
                if colon { Cmp::Ge } else { cmp },
                spec.colors(),
                level,
            ))
        }
        "layout" => require_equality(Query::Layout(value.to_string())),
        // `not:` is Scryfall's inverted `is:`, and it is worth having because
        // `-is:permanent` and `not:permanent` are both typed by people who read
        // the docs.
        "is" | "not" => {
            let want = value.to_ascii_lowercase();
            let property = IS_PROPERTIES
                .iter()
                .find(|(name, _)| *name == want)
                .map(|&(_, p)| p)
                .ok_or_else(|| ParseError::UnknownIsProperty {
                    value: want.clone(),
                })?;
            let q = require_equality(Query::Is(property))?;
            Ok(if key == "not" {
                Query::Not(Box::new(q))
            } else {
                q
            })
        }
        // Every key in `KEYS` has an arm above, and nothing else gets this far.
        // `every_listed_key_parses` holds that; landing here is a key added to
        // the table and not to the match.
        other => unreachable!("search key {other:?} is in KEYS and has no arm"),
    }
}

fn format_term(
    term: &str,
    value: &str,
    status: FormatStatus,
    cmp: Cmp,
    key: &str,
) -> Result<Query, ParseError> {
    if cmp != Cmp::Eq {
        return Err(ParseError::NoComparison {
            key: key.to_string(),
            term: term.to_string(),
        });
    }
    let want = value.to_ascii_lowercase();
    // A closed set, so a misspelling is an error rather than a query that
    // matches nothing. This is the whole reason `Legalities` is a struct.
    let format = FORMATS
        .iter()
        .find(|f| **f == want)
        .ok_or_else(|| ParseError::BadFormat {
            term: term.to_string(),
            value: value.to_string(),
        })?;
    Ok(Query::Format(status, format))
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
