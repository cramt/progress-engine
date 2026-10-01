//! The web host's acceptance check.
//!
//! `engine/tests/engine.rs` pins 6/6 on card name and 5/6 on exact printing
//! for the six fixture frames. The web host runs the same engine through a
//! different execution model, so it is held to the same numbers - and nothing
//! short of a real, cross-origin-isolated browser can run it. `run.sh` builds
//! this, serves it and drives headless Chromium; the page decodes the frames
//! and hands them to [`check`], which fails unless both numbers hold.
//!
//! Empty on native targets, so the workspace still builds and tests there.
#![cfg(target_arch = "wasm32")]

use std::fmt::Write as _;
use std::rc::Rc;

use anyhow::{anyhow, bail, Result};
use gitaxian_probe_engine::{Engine, EngineConfig, Image};
use js_sys::{Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

/// The native test's table: fixture slug, card name, the edition the scanned
/// printing is from.
const CASES: &[(&str, &str, &str)] = &[
    ("lotus", "Black Lotus", "Limited Edition Alpha"),
    ("counterspell", "Counterspell", "Limited Edition Alpha"),
    ("llanowar", "Llanowar Elves", "Core Set 2019"),
    ("shock", "Shock", "Core Set 2021"),
    ("swords", "Swords to Plowshares", "Limited Edition Alpha"),
    ("thoughtseize", "Thoughtseize", "Theros"),
];

/// `frames` maps each slug to `{ rgba: Uint8Array, width, height }`. Resolves
/// to a report the runner prints, or rejects with why the check failed.
#[wasm_bindgen]
pub async fn check(base: String, frames: JsValue) -> Result<String, JsValue> {
    run(base, frames)
        .await
        .map_err(|e| js_sys::Error::new(&format!("{e:#}")).into())
}

async fn run(base: String, frames: JsValue) -> Result<String> {
    let mut report = String::new();
    let stages = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let seen = stages.clone();
    let t0 = now();

    let mut engine = Engine::open(EngineConfig {
        base,
        on_progress: Some(Rc::new(move |p| {
            let mut seen = seen.borrow_mut();
            if seen.last() != Some(&p.stage) {
                seen.push(p.stage);
            }
        })),
        ..EngineConfig::default()
    })
    .await?;
    let boot_ms = now() - t0;
    writeln!(
        report,
        "Delver X {} (fingerprint {}), booted in {boot_ms:.0} ms via {}",
        engine.version(),
        engine.fingerprint(),
        stages.borrow().join(" > ")
    )?;

    let rows = engine.query("SELECT count(*) FROM data.cards").await?;
    writeln!(report, "catalogue: {} cards", rows[0][0])?;

    let lotus = engine
        .query(
            "SELECT c._id FROM data.cards c JOIN data.names n ON n._id = c.name \
             WHERE n.name = 'Black Lotus' LIMIT 1",
        )
        .await?;
    let id: i64 = lotus[0][0].parse()?;
    let card = engine
        .card_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("no card {id}"))?;
    if card.name != "Black Lotus" {
        bail!("card_by_id({id}) is {}, not Black Lotus", card.name);
    }

    let (mut names, mut printings) = (0, 0);
    for (slug, want_name, want_edition) in CASES {
        let frame = Reflect::get(&frames, &(*slug).into()).map_err(js)?;
        if frame.is_undefined() {
            bail!("no frame for {slug}");
        }
        let rgba = Uint8Array::new(&Reflect::get(&frame, &"rgba".into()).map_err(js)?).to_vec();
        let dim = |k: &str| -> Result<u32> {
            Reflect::get(&frame, &k.into())
                .map_err(js)?
                .as_f64()
                .map(|v| v as u32)
                .ok_or_else(|| anyhow!("{slug}: no {k}"))
        };
        let image = Image {
            data: &rgba,
            width: dim("width")?,
            height: dim("height")?,
        };

        if *slug == "lotus" {
            let quad = engine.locate(&image).await?;
            if quad.map_or(0, |q| q.len()) != 4 {
                bail!("locate() found no card quad on the lotus frame");
            }
        }

        let t = now();
        let found = engine.recognize(&image).await?;
        let ms = now() - t;
        let Some(card) = found.first() else {
            bail!("{want_name}: nothing recognised");
        };
        let edition = engine
            .card_by_id(card.data_id)
            .await?
            .map(|c| c.edition)
            .unwrap_or_default();
        if card.name == *want_name {
            names += 1;
            if edition == *want_edition {
                printings += 1;
            }
        }
        writeln!(
            report,
            "  {want_name:<22} -> {} / {edition}  ({ms:.0} ms)",
            card.name
        )?;
    }

    engine.close();
    if engine.query("SELECT 1").await.is_ok() {
        bail!("a closed engine still answered a query");
    }

    write!(
        report,
        "card name {names}/{}, exact printing {printings}/{}",
        CASES.len(),
        CASES.len()
    )?;
    if names != 6 {
        bail!("card-name accuracy regressed\n{report}");
    }
    // The engine's own ceiling, as natively: no OCR, so a same-art reprint
    // (Swords to Plowshares) lands on the wrong printing.
    if printings != 5 {
        bail!("printing accuracy moved - the web host changed behaviour\n{report}");
    }
    Ok(report)
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn js(e: JsValue) -> anyhow::Error {
    anyhow!("{e:?}")
}
