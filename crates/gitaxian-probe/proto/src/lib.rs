//! The line protocol between an app and a `probe-host` process.
//!
//! Its own crate, not a module of the engine: the app needs these types and
//! must not link V8, and depending on the engine crate would link it whether
//! or not anything called it.
//!
//! # Why this is hand-encoded
//!
//! It was `facet-json`, which panics inside `facet-reflect` when deserialising
//! these structs on `aarch64-linux-android`:
//!
//! ```text
//! facet-core-0.46.5/src/types/ptr/mod.rs:182:
//! as_mut_byte_ptr called on wide pointer
//! ```
//!
//! The same binary, the same input and the same code answer correctly on
//! `x86_64-linux-android`, so it is architecture-specific, and the failing
//! frame is `Partial::dealloc` - shared by every facet deserialiser, so
//! msgpack would not dodge it either.
//!
//! Two fields do not need a serialisation library, so this encodes itself. It
//! also keeps the crate dependency-free, which is what stops V8 following the
//! types into the app.
//!
//! One record per line, fields separated by tabs. Tabs and newlines inside a
//! field are escaped, so a field never splits a record.

/// What the app asks of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// `open`, `query` or `close`.
    pub op: String,
    /// Only meaningful for `query`.
    pub sql: String,
}

impl Request {
    pub fn open() -> Self {
        Self {
            op: "open".into(),
            sql: String::new(),
        }
    }

    pub fn query(sql: impl Into<String>) -> Self {
        Self {
            op: "query".into(),
            sql: sql.into(),
        }
    }

    pub fn close() -> Self {
        Self {
            op: "close".into(),
            sql: String::new(),
        }
    }

    pub fn encode(&self) -> String {
        format!("{}\t{}", escape(&self.op), escape(&self.sql))
    }

    pub fn decode(line: &str) -> Self {
        let mut fields = line.trim_end_matches(['\r', '\n']).splitn(2, '\t');
        Self {
            op: unescape(fields.next().unwrap_or_default()),
            sql: unescape(fields.next().unwrap_or_default()),
        }
    }
}

/// What the host answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub ok: bool,
    /// A description on success, the error on failure.
    pub message: String,
    /// Query results, row-major. Empty for every other op.
    pub rows: Vec<Vec<String>>,
}

impl Response {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            rows: Vec::new(),
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            rows: Vec::new(),
        }
    }

    /// `ok|err <tab> message [<tab> row]...`, cells within a row separated by
    /// the unit separator, which no card name or SQL error contains.
    pub fn encode(&self) -> String {
        let mut line = String::from(if self.ok { "ok" } else { "err" });
        line.push('\t');
        line.push_str(&escape(&self.message));
        for row in &self.rows {
            line.push('\t');
            let cells: Vec<String> = row.iter().map(|cell| escape(cell)).collect();
            line.push_str(&cells.join("\u{1f}"));
        }
        line
    }

    pub fn decode(line: &str) -> Result<Self, String> {
        let mut fields = line.trim_end_matches(['\r', '\n']).split('\t');
        let ok = match fields.next() {
            Some("ok") => true,
            Some("err") => false,
            other => return Err(format!("bad status: {other:?}")),
        };
        let message = unescape(fields.next().unwrap_or_default());
        let rows = fields
            .map(|row| row.split('\u{1f}').map(unescape).collect())
            .collect();
        Ok(Self { ok, message, rows })
    }
}

/// Keep field separators out of field contents. Errors from the engine are
/// free-form and do contain newlines.
fn escape(field: &str) -> String {
    field
        .replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\u{1f}', "\\u")
}

fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('u') => out.push('\u{1f}'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips() {
        let request = Request::query("SELECT n.name FROM data.cards");
        assert_eq!(Request::decode(&request.encode()), request);
    }

    #[test]
    fn response_round_trips_rows() {
        let response = Response {
            ok: true,
            message: "2 rows".into(),
            rows: vec![
                vec!["Black Lotus".into(), "R".into()],
                vec!["Mox Pearl".into(), "R".into()],
            ],
        };
        assert_eq!(Response::decode(&response.encode()).unwrap(), response);
    }

    /// Engine errors are multi-line, and a field must never split a record.
    #[test]
    fn separators_inside_fields_survive() {
        let response = Response::err("line one\nline\ttwo");
        let decoded = Response::decode(&response.encode()).unwrap();
        assert_eq!(decoded.message, "line one\nline\ttwo");
        assert!(!response.encode().contains('\n'));
    }
}
