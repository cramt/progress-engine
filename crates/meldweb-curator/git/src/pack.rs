//! Packfiles as fetch receives them and push sends them
//! (gitformat-pack(5)), entirely in memory.
//!
//! Reading resolves every delta, ofs-delta and ref-delta, so the objects can
//! be written loose. gix-pack 0.76 builds for wasm32 with its default
//! features off, but its delta application is crate-private, and its only
//! public way to resolve a pack's deltas is `Bundle::write_to_directory`,
//! which writes a pack and its index to a directory through gix-tempfile.
//! The browser's repo has no directory to write them to, and keeps no packs.
//! So the entry parser and the delta are here, checked against packs real
//! `git` deltified (`tests/remote.rs`).
//!
//! Writing makes undeltified packs: a push from the browser is a few
//! commits of small text files.

use std::collections::HashMap;

use flate2::{Decompress, FlushDecompress, Status};
use gix_hash::ObjectId;
use gix_object::Kind;

use crate::repo::{Error, Result};

const SHA1: gix_hash::Kind = gix_hash::Kind::Sha1;

fn bad(detail: impl std::fmt::Display) -> Error {
    Error::Corrupt {
        what: "the pack".to_owned(),
        detail: detail.to_string(),
    }
}

fn kind_of(code: u8) -> Option<Kind> {
    Some(match code {
        1 => Kind::Commit,
        2 => Kind::Tree,
        3 => Kind::Blob,
        4 => Kind::Tag,
        _ => return None,
    })
}

fn code_of(kind: Kind) -> u8 {
    match kind {
        Kind::Commit => 1,
        Kind::Tree => 2,
        Kind::Blob => 3,
        Kind::Tag => 4,
    }
}

enum Body {
    Whole(Kind, Vec<u8>),
    /// The base starts this many bytes before this entry.
    OfsDelta(u64, Vec<u8>),
    RefDelta(ObjectId, Vec<u8>),
}

/// Inflates exactly `size` bytes from the front of `input`, returning them
/// and how many compressed bytes they took.
fn inflate(input: &[u8], size: usize) -> Result<(Vec<u8>, usize)> {
    let mut z = Decompress::new(true);
    // One byte spare, so the stream's end is read with room to write.
    let mut out = Vec::with_capacity(size + 1);
    loop {
        let before_in = z.total_in();
        let before_out = z.total_out();
        let status = z
            .decompress_vec(
                &input[z.total_in() as usize..],
                &mut out,
                FlushDecompress::None,
            )
            .map_err(bad)?;
        if status == Status::StreamEnd {
            break;
        }
        if out.len() == out.capacity() {
            // A stream that claims to be longer than its header says.
            out.reserve(64);
        }
        if z.total_in() == before_in && z.total_out() == before_out {
            return Err(bad("an entry's compressed data ends early"));
        }
    }
    if out.len() != size {
        return Err(bad(format!(
            "an entry inflates to {} bytes but its header says {size}",
            out.len()
        )));
    }
    Ok((out, z.total_in() as usize))
}

/// Every object in `pack`, deltas resolved, each with its id. A ref-delta
/// whose base is not in the pack is looked up with `outside`, so a thin
/// pack's bases may come from the repo.
pub(crate) fn read(
    pack: &[u8],
    outside: impl Fn(&ObjectId) -> Result<Option<(Kind, Vec<u8>)>>,
) -> Result<Vec<(ObjectId, Kind, Vec<u8>)>> {
    if pack.len() < 32 || &pack[..4] != b"PACK" {
        return Err(bad("it does not start with PACK"));
    }
    let version = u32::from_be_bytes(pack[4..8].try_into().expect("4 bytes"));
    if version != 2 && version != 3 {
        return Err(bad(format!("version {version} is not one git writes")));
    }
    let count = u32::from_be_bytes(pack[8..12].try_into().expect("4 bytes")) as usize;
    let (body, trailer) = pack.split_at(pack.len() - 20);
    let mut hasher = gix_hash::hasher(SHA1);
    hasher.update(body);
    let sum = hasher.try_finalize().map_err(bad)?;
    if sum.as_slice() != trailer {
        return Err(bad("its checksum does not match its contents"));
    }

    let mut entries: Vec<(u64, Body)> = Vec::with_capacity(count);
    let mut at = 12usize;
    for _ in 0..count {
        let start = at;
        let byte = |i: usize| {
            body.get(i)
                .copied()
                .ok_or_else(|| bad("it ends inside an entry"))
        };
        let mut c = byte(at)?;
        at += 1;
        let code = (c >> 4) & 7;
        let mut size = u64::from(c & 0x0f);
        let mut shift = 4;
        while c & 0x80 != 0 {
            c = byte(at)?;
            at += 1;
            size |= u64::from(c & 0x7f) << shift;
            shift += 7;
            if shift > 63 {
                return Err(bad("an entry's size does not fit 64 bits"));
            }
        }
        let size = usize::try_from(size).map_err(bad)?;
        let entry = match code {
            6 => {
                let mut c = byte(at)?;
                at += 1;
                let mut back = u64::from(c & 0x7f);
                while c & 0x80 != 0 {
                    c = byte(at)?;
                    at += 1;
                    back = ((back + 1) << 7) | u64::from(c & 0x7f);
                }
                let (data, used) = inflate(&body[at..], size)?;
                at += used;
                Body::OfsDelta(back, data)
            }
            7 => {
                let id = body
                    .get(at..at + 20)
                    .ok_or_else(|| bad("it ends inside a ref-delta's base id"))?;
                let id = ObjectId::from_bytes_or_panic(id);
                at += 20;
                let (data, used) = inflate(&body[at..], size)?;
                at += used;
                Body::RefDelta(id, data)
            }
            code => {
                let kind = kind_of(code).ok_or_else(|| bad(format!("entry type {code}")))?;
                let (data, used) = inflate(&body[at..], size)?;
                at += used;
                Body::Whole(kind, data)
            }
        };
        entries.push((start as u64, entry));
    }
    if at != body.len() {
        return Err(bad("there are bytes after its last entry"));
    }

    let at_offset: HashMap<u64, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, (offset, _))| (*offset, i))
        .collect();
    let mut resolved: Vec<Option<(Kind, Vec<u8>)>> = vec![None; entries.len()];
    let mut by_id: HashMap<ObjectId, usize> = HashMap::new();
    for (i, (_, entry)) in entries.iter().enumerate() {
        if let Body::Whole(kind, data) = entry {
            let id = gix_object::compute_hash(SHA1, *kind, data).map_err(bad)?;
            by_id.insert(id, i);
            resolved[i] = Some((*kind, data.clone()));
        }
    }
    // A delta's base may itself be a delta, and a ref-delta's base may come
    // later in the pack, so resolve until a pass makes no progress.
    loop {
        let mut progressed = false;
        let mut waiting = false;
        for i in 0..entries.len() {
            if resolved[i].is_some() {
                continue;
            }
            let (offset, entry) = &entries[i];
            let base: Option<(Kind, Vec<u8>)> = match entry {
                Body::Whole(..) => unreachable!("resolved above"),
                Body::OfsDelta(back, _) => {
                    let base_at = offset
                        .checked_sub(*back)
                        .ok_or_else(|| bad("an ofs-delta points before the pack"))?;
                    let j = *at_offset
                        .get(&base_at)
                        .ok_or_else(|| bad("an ofs-delta points between entries"))?;
                    resolved[j].clone()
                }
                Body::RefDelta(id, _) => match by_id.get(id) {
                    Some(&j) => resolved[j].clone(),
                    None => outside(id)?,
                },
            };
            let Some((kind, base)) = base else {
                waiting = true;
                continue;
            };
            let (Body::OfsDelta(_, delta) | Body::RefDelta(_, delta)) = entry else {
                unreachable!("only deltas wait")
            };
            let data = apply_delta(&base, delta)?;
            let id = gix_object::compute_hash(SHA1, kind, &data).map_err(bad)?;
            by_id.insert(id, i);
            resolved[i] = Some((kind, data));
            progressed = true;
        }
        if !waiting {
            break;
        }
        if !progressed {
            return Err(bad("a delta's base is neither in the pack nor in the repo"));
        }
    }
    let mut out = Vec::with_capacity(entries.len());
    for (id, i) in by_id {
        let (kind, data) = resolved[i].take().expect("every entry resolved");
        out.push((id, kind, data));
    }
    out.sort_by_key(|(id, ..)| *id);
    Ok(out)
}

fn varint(delta: &[u8], at: &mut usize) -> Result<usize> {
    let mut n = 0usize;
    let mut shift = 0;
    loop {
        let c = *delta
            .get(*at)
            .ok_or_else(|| bad("a delta ends in its header"))?;
        *at += 1;
        n |= usize::from(c & 0x7f) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Ok(n);
        }
        if shift > 56 {
            return Err(bad("a delta's size does not fit"));
        }
    }
}

/// `base` with git's delta applied: a header of the base's and the result's
/// sizes, then copies from the base and inserts of literal bytes.
pub(crate) fn apply_delta(base: &[u8], delta: &[u8]) -> Result<Vec<u8>> {
    let mut at = 0;
    let base_size = varint(delta, &mut at)?;
    if base_size != base.len() {
        return Err(bad(format!(
            "a delta wants a base of {base_size} bytes and has {}",
            base.len()
        )));
    }
    let size = varint(delta, &mut at)?;
    let mut out = Vec::with_capacity(size);
    while at < delta.len() {
        let op = delta[at];
        at += 1;
        if op & 0x80 != 0 {
            let mut field = |bits: u8, from: usize| -> Result<usize> {
                let mut n = 0usize;
                for i in 0..bits as usize {
                    if op & (1 << (from + i)) != 0 {
                        let b = *delta.get(at).ok_or_else(|| bad("a copy ends early"))?;
                        at += 1;
                        n |= usize::from(b) << (8 * i);
                    }
                }
                Ok(n)
            };
            let offset = field(4, 0)?;
            let mut len = field(3, 4)?;
            if len == 0 {
                len = 0x10000;
            }
            let piece = offset
                .checked_add(len)
                .and_then(|end| base.get(offset..end))
                .ok_or_else(|| bad("a copy reaches past its base"))?;
            out.extend_from_slice(piece);
        } else if op != 0 {
            let piece = delta
                .get(at..at + op as usize)
                .ok_or_else(|| bad("an insert ends early"))?;
            out.extend_from_slice(piece);
            at += op as usize;
        } else {
            return Err(bad("a delta holds the reserved instruction 0"));
        }
    }
    if out.len() != size {
        return Err(bad(format!(
            "a delta makes {} bytes but says {size}",
            out.len()
        )));
    }
    Ok(out)
}

/// A version 2 pack of `objects`, none deltified.
pub(crate) fn write(objects: &[(Kind, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"PACK");
    out.extend_from_slice(&2u32.to_be_bytes());
    out.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    for (kind, data) in objects {
        let mut size = data.len() as u64;
        let mut c = (code_of(*kind) << 4) | (size & 0x0f) as u8;
        size >>= 4;
        while size != 0 {
            out.push(c | 0x80);
            c = (size & 0x7f) as u8;
            size >>= 7;
        }
        out.push(c);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, data).expect("writing to a Vec cannot fail");
        out.extend(z.finish().expect("writing to a Vec cannot fail"));
    }
    let mut hasher = gix_hash::hasher(SHA1);
    hasher.update(&out);
    let sum = hasher.try_finalize().expect("sha1dc of our own pack");
    out.extend_from_slice(sum.as_slice());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_pack_reads_back() {
        let objects = vec![
            (Kind::Blob, b"hello\n".to_vec()),
            (Kind::Blob, vec![b'x'; 5000]),
        ];
        let pack = write(&objects);
        let read = read(&pack, |_| Ok(None)).unwrap();
        let mut datas: Vec<_> = read.into_iter().map(|(_, k, d)| (k, d)).collect();
        datas.sort_by_key(|(_, d)| d.len());
        assert_eq!(datas, objects);
    }

    #[test]
    fn a_delta_copies_and_inserts() {
        let base = b"0123456789";
        // base 10, result 7; copy offset 2 len 3; insert "ab"; copy offset 8 len 2.
        let delta = [10, 7, 0x91, 2, 3, 2, b'a', b'b', 0x91, 8, 2];
        assert_eq!(apply_delta(base, &delta).unwrap(), b"234ab89");
    }

    #[test]
    fn a_tampered_pack_is_refused() {
        let mut pack = write(&[(Kind::Blob, b"hi".to_vec())]);
        let n = pack.len();
        pack[n - 21] ^= 1;
        assert!(read(&pack, |_| Ok(None)).is_err());
    }
}
