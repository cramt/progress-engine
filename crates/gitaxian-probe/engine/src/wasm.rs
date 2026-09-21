//! Minimal WASM binary surgery: the import fingerprint that guards against ABI
//! drift, and the +18-byte patch that exposes the module's two internal tags.
//!
//! Ported from `lib/wasm-sections.js`, `lib/wasm-imports.js` and
//! `lib/patch-tags.js`; the fingerprint is byte-compatible with the JS one, so
//! both harnesses agree on whether a build is the verified one.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

const SECTION_IMPORT: u8 = 2;
const SECTION_EXPORT: u8 = 7;
const SECTION_TAG: u8 = 13;
const EXPORT_KIND_TAG: u8 = 0x04;

pub struct Section {
    pub id: u8,
    pub size_start: usize,
    pub payload_start: usize,
    pub payload_end: usize,
}

fn read_leb(buf: &[u8], mut pos: usize) -> Result<(u32, usize)> {
    let (mut result, mut shift) = (0u32, 0u32);
    loop {
        let byte = *buf
            .get(pos)
            .context("LEB128 ran off the end of the buffer")?;
        pos += 1;
        result |= ((byte & 0x7f) as u32) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return Ok((result, pos));
        }
    }
}

fn write_leb(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return;
        }
    }
}

pub fn parse_sections(buf: &[u8]) -> Result<Vec<Section>> {
    if buf.len() < 8 || &buf[0..4] != b"\0asm" {
        bail!("not a wasm module");
    }
    let mut sections = Vec::new();
    let mut pos = 8;
    while pos < buf.len() {
        let id = buf[pos];
        let size_start = pos + 1;
        let (size, payload_start) = read_leb(buf, size_start)?;
        sections.push(Section {
            id,
            size_start,
            payload_start,
            payload_end: payload_start + size as usize,
        });
        pos = payload_start + size as usize;
    }
    Ok(sections)
}

struct Limits {
    min: u32,
    max: Option<u32>,
    shared: bool,
}

enum ImportKind {
    Func(u32),
    Memory(Limits),
    Tag,
    Other,
}

fn read_name(buf: &[u8], pos: usize) -> Result<usize> {
    let (len, start) = read_leb(buf, pos)?;
    Ok(start + len as usize)
}

fn read_limits(buf: &[u8], pos: usize) -> Result<(Limits, usize)> {
    let (flags, p1) = read_leb(buf, pos)?;
    let (min, p2) = read_leb(buf, p1)?;
    let shared = flags & 0x02 != 0;
    if flags & 0x01 != 0 {
        let (max, p3) = read_leb(buf, p2)?;
        Ok((
            Limits {
                min,
                max: Some(max),
                shared,
            },
            p3,
        ))
    } else {
        Ok((
            Limits {
                min,
                max: None,
                shared,
            },
            p2,
        ))
    }
}

fn parse_imports(buf: &[u8]) -> Result<Vec<ImportKind>> {
    let sections = parse_sections(buf)?;
    let Some(sec) = sections.iter().find(|s| s.id == SECTION_IMPORT) else {
        return Ok(Vec::new());
    };
    let (count, mut pos) = read_leb(buf, sec.payload_start)?;
    let mut imports = Vec::with_capacity(count as usize);
    for i in 0..count {
        pos = read_name(buf, pos)?; // module
        pos = read_name(buf, pos)?; // field
        let kind = buf[pos];
        pos += 1;
        imports.push(match kind {
            0 => {
                let (ty, p) = read_leb(buf, pos)?;
                pos = p;
                ImportKind::Func(ty)
            }
            1 => {
                pos += 1; // reftype
                let (limits, p) = read_limits(buf, pos)?;
                pos = p;
                let _ = limits;
                ImportKind::Other
            }
            2 => {
                let (limits, p) = read_limits(buf, pos)?;
                pos = p;
                ImportKind::Memory(limits)
            }
            3 => {
                pos += 2; // valtype + mutability
                ImportKind::Other
            }
            4 => {
                pos += 1; // attribute
                let (_ty, p) = read_leb(buf, pos)?;
                pos = p;
                ImportKind::Tag
            }
            other => bail!("unknown import kind {other} at import #{i}"),
        });
    }
    Ok(imports)
}

/// FINDINGS.md §5: the minified import *names* are positional, so binding to
/// them is one inserted import away from silently misrouting every call. This
/// hashes arity, the type histogram and the memory limits instead, which only
/// move when the C++ feature set or emsdk version does.
pub fn import_fingerprint(buf: &[u8]) -> Result<String> {
    let imports = parse_imports(buf)?;
    let mut types: Vec<(u32, u32)> = Vec::new();
    let mut memory = None;
    for import in &imports {
        match import {
            ImportKind::Func(ty) => match types.iter_mut().find(|(t, _)| t == ty) {
                Some((_, count)) => *count += 1,
                None => types.push((*ty, 1)),
            },
            ImportKind::Memory(limits) => memory = Some(limits),
            _ => {}
        }
    }
    let func_count: u32 = types.iter().map(|(_, c)| c).sum();
    types.sort_by_key(|(t, _)| *t);

    let types_json = types
        .iter()
        .map(|(t, c)| format!("[{t},{c}]"))
        .collect::<Vec<_>>()
        .join(",");
    let memory_json = match memory {
        Some(m) => format!(
            "[{},{},{}]",
            m.min,
            m.max.map_or("null".to_string(), |v| v.to_string()),
            m.shared
        ),
        None => "null".to_string(),
    };
    // Byte-for-byte the JSON.stringify shape lib/wasm-imports.js hashes.
    let canonical =
        format!(r#"{{"funcCount":{func_count},"types":[{types_json}],"memory":{memory_json}}}"#);

    let digest = Sha256::digest(canonical.as_bytes());
    Ok(digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..16]
        .to_string())
}

/// FINDINGS.md §10: `core.wasm` defines two tags and exports neither, so a
/// thrown C++ exception has no `WebAssembly.Tag` to be read against. Appending
/// two export entries makes `e.is(tag)` / `e.getArg(tag, 0)` work. The module on
/// disk is never touched — this rewrites the bytes on the way into the isolate.
pub fn export_internal_tags(buf: &[u8]) -> Result<Vec<u8>> {
    let sections = parse_sections(buf)?;
    let tag_sec = sections
        .iter()
        .find(|s| s.id == SECTION_TAG)
        .context("module has no tag section")?;
    let exp_sec = sections
        .iter()
        .find(|s| s.id == SECTION_EXPORT)
        .context("module has no export section")?;

    let (defined_tags, _) = read_leb(buf, tag_sec.payload_start)?;
    // Imported tags occupy the low indices; defined tags follow.
    let imported_tags = parse_imports(buf)?
        .iter()
        .filter(|i| matches!(i, ImportKind::Tag))
        .count() as u32;

    let (export_count, entries_start) = read_leb(buf, exp_sec.payload_start)?;

    let mut payload = Vec::new();
    write_leb(export_count + defined_tags, &mut payload);
    payload.extend_from_slice(&buf[entries_start..exp_sec.payload_end]);
    for i in 0..defined_tags {
        let name = format!("__tag{i}");
        write_leb(name.len() as u32, &mut payload);
        payload.extend_from_slice(name.as_bytes());
        payload.push(EXPORT_KIND_TAG);
        write_leb(imported_tags + i, &mut payload);
    }

    let mut out = Vec::with_capacity(buf.len() + 32);
    out.extend_from_slice(&buf[..exp_sec.size_start - 1]);
    out.push(exp_sec.id);
    write_leb(payload.len() as u32, &mut out);
    out.extend_from_slice(&payload);
    out.extend_from_slice(&buf[exp_sec.payload_end..]);
    Ok(out)
}
