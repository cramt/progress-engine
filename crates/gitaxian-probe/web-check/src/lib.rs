//! The web host's acceptance check.
//!
//! `engine/tests/engine.rs` pins 6/6 on card name for all three tiers, and
//! 5/6 on exact printing for alpha, 4/6 for lambda and gamma - a real gap
//! between the tiers' weights (the alpha-only miss is Swords to Plowshares, a
//! same-art reprint with no OCR to separate it; lambda and gamma also miss
//! Counterspell's printing). The web host runs the same engine through a
//! different execution model, so each tier is held to its own number - and
//! nothing short of a real, cross-origin-isolated browser can run it. `run.sh`
//! builds this, serves it and drives headless Chromium once per tier; the
//! page decodes the frames and hands them to [`check`], which fails unless
//! both numbers hold for the tier it booted.
//!
//! Empty on native targets, so the workspace still builds and tests there.
#![cfg(target_arch = "wasm32")]

use std::fmt::Write as _;
use std::rc::Rc;

use anyhow::{anyhow, bail, Result};
use gitaxian_probe_engine::{Engine, EngineConfig, Image, Tier};
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

/// Each tier's own ceiling on the six fixtures (card name, exact printing),
/// measured natively and held here rather than one shared number.
fn ceiling(tier: Tier) -> (u32, u32) {
    match tier {
        Tier::Alpha => (6, 5),
        Tier::Lambda => (6, 4),
        Tier::Gamma => (6, 4),
    }
}

/// `frames` maps each slug to `{ rgba: Uint8Array, width, height }`. Resolves
/// to a report the runner prints, or rejects with why the check failed.
#[wasm_bindgen]
pub async fn check(base: String, tier: String, frames: JsValue) -> Result<String, JsValue> {
    run(base, tier, frames)
        .await
        .map_err(|e| js_sys::Error::new(&format!("{e:#}")).into())
}

async fn run(base: String, tier: String, frames: JsValue) -> Result<String> {
    let model = Tier::from_name(&tier)?;
    let mut report = String::new();
    let stages = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let seen = stages.clone();
    let t0 = now();

    let mut engine = Engine::open(EngineConfig {
        base,
        model,
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
        "Delver X {} (fingerprint {}), tier {tier}, booted in {boot_ms:.0} ms via {}",
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
    let (want_names, want_printings) = ceiling(model);
    if names != want_names {
        bail!("card-name accuracy regressed for {tier}\n{report}");
    }
    // Each tier's own ceiling, as measured natively: no OCR, so a same-art
    // reprint lands on the wrong printing, and lambda/gamma miss one more
    // than alpha does.
    if printings != want_printings {
        bail!("printing accuracy moved for {tier} - the web host changed behaviour\n{report}");
    }
    Ok(report)
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn js(e: JsValue) -> anyhow::Error {
    anyhow!("{e:?}")
}
