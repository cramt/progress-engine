//! git's pkt-line framing (gitprotocol-common(5)): four hex digits of length,
//! counting themselves, then the payload; `0000` is a flush. Written here
//! rather than taken from gix-packetline, whose framing lives behind its
//! blocking or async IO features, because this crate only ever holds whole
//! request and response bodies in memory.

/// The largest payload one pkt-line carries: 65520 bytes less the four of
/// its length.
pub(crate) const MAX_DATA: usize = 65516;

/// One frame of a pkt-line stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line<'a> {
    Data(&'a [u8]),
    Flush,
    /// `0001`, protocol v2's delimiter.
    Delim,
    /// `0002`, protocol v2's response end.
    ResponseEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PktError {
    #[error("the stream ends inside a pkt-line")]
    Truncated,
    #[error("{0:?} is not a pkt-line length")]
    BadLength(String),
}

pub(crate) fn data(out: &mut Vec<u8>, payload: &[u8]) {
    assert!(
        payload.len() <= MAX_DATA,
        "a pkt-line holds at most 65516 bytes"
    );
    out.extend_from_slice(format!("{:04x}", payload.len() + 4).as_bytes());
    out.extend_from_slice(payload);
}

pub(crate) fn text(out: &mut Vec<u8>, line: &str) {
    data(out, line.as_bytes());
}

pub(crate) fn flush(out: &mut Vec<u8>) {
    out.extend_from_slice(b"0000");
}

/// Reads pkt-lines from the front of a buffer, leaving whatever follows the
/// last one read, which for upload-pack without side-band is the raw pack.
pub(crate) struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    pub(crate) fn rest(&self) -> &'a [u8] {
        self.rest
    }

    /// The next frame, or `None` at the end of the buffer.
    pub(crate) fn next_line(&mut self) -> Result<Option<Line<'a>>, PktError> {
        if self.rest.is_empty() {
            return Ok(None);
        }
        let Some(head) = self.rest.get(..4) else {
            return Err(PktError::Truncated);
        };
        let len = std::str::from_utf8(head)
            .ok()
            .and_then(|h| usize::from_str_radix(h, 16).ok())
            .ok_or_else(|| PktError::BadLength(String::from_utf8_lossy(head).into_owned()))?;
        let line = match len {
            0 => Line::Flush,
            1 => Line::Delim,
            2 => Line::ResponseEnd,
            3 => return Err(PktError::BadLength("0003".to_owned())),
            n => {
                let payload = self.rest.get(4..n).ok_or(PktError::Truncated)?;
                self.rest = &self.rest[n..];
                return Ok(Some(Line::Data(payload)));
            }
        };
        self.rest = &self.rest[4..];
        Ok(Some(line))
    }

    /// Whether the next bytes look like a pkt-line length rather than a raw
    /// pack, which starts `PACK`.
    pub(crate) fn at_pkt_line(&self) -> bool {
        self.rest.len() >= 4 && self.rest[..4].iter().all(u8::is_ascii_hexdigit)
    }
}

/// A text line's payload without the newline git ends it with.
pub(crate) fn chomp(payload: &[u8]) -> &[u8] {
    payload.strip_suffix(b"\n").unwrap_or(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_leave_what_follows() {
        let mut out = Vec::new();
        text(&mut out, "want abc\n");
        flush(&mut out);
        out.extend_from_slice(b"PACKraw");
        assert_eq!(&out[..4], b"000d");
        let mut r = Reader::new(&out);
        assert_eq!(r.next_line().unwrap(), Some(Line::Data(b"want abc\n")));
        assert_eq!(r.next_line().unwrap(), Some(Line::Flush));
        assert!(!r.at_pkt_line());
        assert_eq!(r.rest(), b"PACKraw");
    }

    #[test]
    fn a_short_stream_is_refused() {
        assert_eq!(Reader::new(b"000ahi").next_line(), Err(PktError::Truncated));
        assert!(matches!(
            Reader::new(b"zzzz").next_line(),
            Err(PktError::BadLength(_))
        ));
    }
}
