//! `src/walk.rs` is generated from `src/ast.rs`: every type that can hold
//! an `Objects` gets a `Walk` impl visiting its fields in declaration order,
//! which is the order the text says them in. The AST has some five hundred
//! variants, so the visitor is written by this test, not by hand, and the
//! test fails when the two drift. `UPDATE_WALK=1 cargo test -p
//! vivisurgeons-insight --test walk` writes it again.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Debug)]
enum Shape {
    Struct(Vec<(String, String)>),
    Tuple(Vec<String>),
    Enum(Vec<(String, Variant)>),
}

#[derive(Debug)]
enum Variant {
    Unit,
    Tuple(Vec<String>),
    Struct(Vec<(String, String)>),
}

/// Split at commas outside any `<>` or `()`.
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for c in s.chars() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn field(line: &str) -> Option<(String, String)> {
    let l = line.trim().trim_start_matches("pub ").trim_end_matches(',');
    let (name, ty) = l.split_once(": ")?;
    Some((name.to_string(), ty.to_string()))
}

fn parse(src: &str) -> BTreeMap<String, Shape> {
    let mut types = BTreeMap::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if let Some(rest) = l.strip_prefix("pub struct ") {
            if let Some((name, inner)) = rest.split_once('(') {
                let inner = inner.trim_end_matches(");");
                let tys = split_top(inner)
                    .into_iter()
                    .map(|t| t.trim_start_matches("pub ").to_string())
                    .collect();
                types.insert(name.to_string(), Shape::Tuple(tys));
            } else {
                let name = rest.trim_end_matches(" {").to_string();
                let mut fields = Vec::new();
                i += 1;
                while lines[i] != "}" {
                    if let Some(f) = field(lines[i]).filter(|_| !lines[i].trim().starts_with("//"))
                    {
                        fields.push(f);
                    }
                    i += 1;
                }
                types.insert(name, Shape::Struct(fields));
            }
        } else if let Some(rest) = l.strip_prefix("pub enum ") {
            let name = rest.trim_end_matches(" {").to_string();
            let mut variants = Vec::new();
            i += 1;
            while lines[i] != "}" {
                let v = lines[i];
                let t = v.trim();
                if t.is_empty() || t.starts_with("//") || t.starts_with("#[") {
                    i += 1;
                    continue;
                }
                if let Some(head) = t.strip_suffix(" {") {
                    let mut fields = Vec::new();
                    i += 1;
                    while lines[i].trim() != "}," {
                        let f = lines[i].trim();
                        if !f.starts_with("//") && !f.is_empty() {
                            fields.push(field(f).expect(f));
                        }
                        i += 1;
                    }
                    variants.push((head.to_string(), Variant::Struct(fields)));
                } else if let Some((head, inner)) = t.split_once('(') {
                    let inner = inner.trim_end_matches(',').trim_end_matches(')');
                    variants.push((head.to_string(), Variant::Tuple(split_top(inner))));
                } else {
                    let head = t.trim_end_matches(',').split(" = ").next().unwrap();
                    variants.push((head.to_string(), Variant::Unit));
                }
                i += 1;
            }
            types.insert(name, Shape::Enum(variants));
        }
        i += 1;
    }
    types
}

/// The named types a type expression mentions: "Vec<(Amount, Trigger)>".
fn mentions(ty: &str) -> Vec<String> {
    ty.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.starts_with(char::is_uppercase))
        .map(str::to_string)
        .collect()
}

fn walkable(types: &BTreeMap<String, Shape>) -> BTreeSet<String> {
    let mut set: BTreeSet<String> = ["Objects".to_string()].into();
    let field_types = |s: &Shape| -> Vec<String> {
        match s {
            Shape::Struct(f) => f.iter().map(|(_, t)| t.clone()).collect(),
            Shape::Tuple(t) => t.clone(),
            Shape::Enum(v) => v
                .iter()
                .flat_map(|(_, v)| match v {
                    Variant::Unit => vec![],
                    Variant::Tuple(t) => t.clone(),
                    Variant::Struct(f) => f.iter().map(|(_, t)| t.clone()).collect(),
                })
                .collect(),
        }
    };
    loop {
        let before = set.len();
        for (name, shape) in types {
            if field_types(shape)
                .iter()
                .any(|t| mentions(t).iter().any(|m| set.contains(m)))
            {
                set.insert(name.clone());
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

/// Code that walks `expr` (a `&mut` to a value of type `ty`), or nothing.
fn walk_expr(ty: &str, expr: &str, set: &BTreeSet<String>, depth: usize) -> Option<String> {
    let ty = ty.trim();
    if !mentions(ty).iter().any(|m| set.contains(m)) {
        return None;
    }
    if let Some(inner) = ty.strip_prefix("Box<").and_then(|t| t.strip_suffix('>')) {
        return walk_expr(inner, &format!("&mut **{expr}"), set, depth);
    }
    if let Some(inner) = ty.strip_prefix("Vec<").and_then(|t| t.strip_suffix('>')) {
        let x = format!("x{depth}");
        let body = walk_expr(inner, &x, set, depth + 1)?;
        return Some(format!("for {x} in ({expr}).iter_mut() {{ {body} }}"));
    }
    if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
        let x = format!("x{depth}");
        let body = walk_expr(inner, &x, set, depth + 1)?;
        return Some(format!("if let Some({x}) = ({expr}).as_mut() {{ {body} }}"));
    }
    if let Some(inner) = ty.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        let parts = split_top(inner);
        let names: Vec<String> = (0..parts.len()).map(|k| format!("t{depth}_{k}")).collect();
        let body: Vec<String> = parts
            .iter()
            .zip(&names)
            .filter_map(|(t, n)| walk_expr(t, n, set, depth + 1))
            .collect();
        let names: Vec<String> = parts
            .iter()
            .zip(names)
            .map(|(t, n)| {
                if walk_expr(t, &n, set, depth + 1).is_some() {
                    n
                } else {
                    "_".into()
                }
            })
            .collect();
        return Some(format!(
            "{{ let ({}) = {expr}; {} }}",
            names.join(", "),
            body.join(" ")
        ));
    }
    set.contains(ty).then(|| format!("Walk::walk({expr}, v);"))
}

fn generate(src: &str) -> String {
    let types = parse(src);
    let set = walkable(&types);
    let mut out = String::from(
        "//! Generated by `tests/walk.rs` from `src/ast.rs`; do not edit.\n\
         //! `UPDATE_WALK=1 cargo test -p vivisurgeons-insight --test walk`\n\
         //! writes it again.\n\n\
         use crate::ast::*;\n\n\
         /// What a walk does at each object, in the order the text says them.\n\
         /// A nested ability is a scope of its own, so a walk enters one only\n\
         /// when `ability` says so.\n\
         pub trait Visitor {\n    \
             /// Before the object's own parts; `false` skips them.\n    \
             fn enter(&mut self, o: &mut Objects) -> bool;\n    \
             /// After them.\n    \
             fn leave(&mut self, _o: &mut Objects) {}\n    \
             fn ability(&mut self, _a: &mut Ability) -> bool {\n        false\n    }\n    \
             /// Before and after an effect's parts.\n    \
             fn effect(&mut self, _e: &mut Effect) {}\n    \
             fn effect_done(&mut self, _e: &mut Effect) {}\n    \
             /// Before and after a condition's parts.\n    \
             fn condition(&mut self, _c: &mut Condition) {}\n    \
             fn condition_done(&mut self, _c: &mut Condition) {}\n\
         }\n\n\
         pub trait Walk {\n    fn walk(&mut self, v: &mut dyn Visitor);\n}\n",
    );
    for (name, shape) in &types {
        if !set.contains(name) {
            continue;
        }
        let body = match shape {
            Shape::Struct(fields) => fields
                .iter()
                .filter_map(|(f, t)| walk_expr(t, &format!("&mut self.{f}"), &set, 0))
                .collect::<Vec<_>>()
                .join("\n"),
            Shape::Tuple(tys) => tys
                .iter()
                .enumerate()
                .filter_map(|(k, t)| walk_expr(t, &format!("&mut self.{k}"), &set, 0))
                .collect::<Vec<_>>()
                .join("\n"),
            Shape::Enum(variants) => {
                let mut arms = String::new();
                let mut rest = false;
                for (vname, v) in variants {
                    match v {
                        Variant::Unit => rest = true,
                        Variant::Tuple(tys) => {
                            let binds: Vec<String> =
                                (0..tys.len()).map(|k| format!("f{k}")).collect();
                            let body: Vec<String> = tys
                                .iter()
                                .zip(&binds)
                                .filter_map(|(t, b)| walk_expr(t, b, &set, 0))
                                .collect();
                            if body.is_empty() {
                                rest = true;
                                continue;
                            }
                            let pat: Vec<String> = tys
                                .iter()
                                .zip(&binds)
                                .map(|(t, b)| {
                                    if walk_expr(t, b, &set, 0).is_some() {
                                        b.clone()
                                    } else {
                                        "_".into()
                                    }
                                })
                                .collect();
                            writeln!(
                                arms,
                                "{name}::{vname}({}) => {{ {} }}",
                                pat.join(", "),
                                body.join(" ")
                            )
                            .unwrap();
                        }
                        Variant::Struct(fields) => {
                            let used: Vec<&(String, String)> = fields
                                .iter()
                                .filter(|(f, t)| walk_expr(t, f, &set, 0).is_some())
                                .collect();
                            if used.is_empty() {
                                rest = true;
                                continue;
                            }
                            let body: Vec<String> = used
                                .iter()
                                .filter_map(|(f, t)| walk_expr(t, f, &set, 0))
                                .collect();
                            let names: Vec<&str> = used.iter().map(|(f, _)| f.as_str()).collect();
                            let dots = if used.len() < fields.len() {
                                ", .."
                            } else {
                                ""
                            };
                            writeln!(
                                arms,
                                "{name}::{vname} {{ {}{dots} }} => {{ {} }}",
                                names.join(", "),
                                body.join(" ")
                            )
                            .unwrap();
                        }
                    }
                }
                if rest {
                    arms.push_str("_ => {}\n");
                }
                format!("match self {{\n{arms}}}")
            }
        };
        let body = match name.as_str() {
            "Objects" => format!("if v.enter(self) {{\n{body}\n}}\nv.leave(self);"),
            "Ability" => format!("if !v.ability(self) {{\nreturn;\n}}\n{body}"),
            "Effect" => format!("v.effect(self);\n{body}\nv.effect_done(self);"),
            "Condition" => format!("v.condition(self);\n{body}\nv.condition_done(self);"),
            _ => body,
        };
        write!(
            out,
            "\nimpl Walk for {name} {{\n#[allow(clippy::all)]\nfn walk(&mut self, v: &mut dyn Visitor) {{\n{body}\n}}\n}}\n"
        )
        .unwrap();
    }
    out
}

#[test]
fn walk_rs_is_generated_from_the_ast() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ast = std::fs::read_to_string(format!("{dir}/src/ast.rs")).unwrap();
    let path = format!("{dir}/src/walk.rs");
    let want = generate(&ast);
    if std::env::var_os("UPDATE_WALK").is_some() {
        // `cargo fmt` formats it after; the comparison ignores whitespace.
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    // rustfmt adds whitespace and trailing commas, which say nothing.
    let squeeze = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .replace(",}", "}")
            .replace(",)", ")")
            .replace(",]", "]")
    };
    assert!(
        squeeze(&have) == squeeze(&want),
        "src/walk.rs is stale: run UPDATE_WALK=1 cargo test -p vivisurgeons-insight --test walk, then cargo fmt"
    );
}
