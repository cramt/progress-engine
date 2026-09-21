//! End-to-end against the real engine, inside the sandbox.
//!
//! The engine downloads itself on first use and is cached from then on, so the
//! only reason to skip is having neither a network nor a cache.
//!
//! One engine for the whole run: booting costs ~5s and 32 OS threads, so the
//! cases share an instance rather than each paying for their own.

mod skip;

use std::path::{Path, PathBuf};
use std::process::Command;

use gitaxian_probe_engine::{
    Bundle, Engine, EngineConfig, Image, Model, Source, Tier, KNOWN_FINGERPRINT,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether the engine's files can be had at all. A fetch that works but an
/// engine that will not boot is a failure, not a skip.
fn ready() -> bool {
    match Bundle::fetch(&Source::default(), Tier::Alpha, None) {
        Ok(_) => true,
        Err(e) => {
            skip::skipped(&format!("the engine could not be fetched: {e:#}"));
            false
        }
    }
}

fn to_rgba(file: &Path) -> Option<(Vec<u8>, u32, u32)> {
    let dims = Command::new("magick")
        .args(["identify", "-format", "%w %h"])
        .arg(file)
        .output()
        .ok()?;
    if !dims.status.success() {
        return None;
    }
    let dims = String::from_utf8(dims.stdout).ok()?;
    let (w, h) = dims.trim().split_once(' ')?;
    let data = Command::new("magick")
        .arg(file)
        .args(["-depth", "8", "RGBA:-"])
        .output()
        .ok()?;
    Some((data.stdout, w.parse().ok()?, h.parse().ok()?))
}

#[test]
fn engine_boots_queries_and_recognises() {
    if !ready() {
        return;
    }
    let mut engine = Engine::open(EngineConfig {
        model: Model::Alpha,
        ..Default::default()
    })
    .expect("engine should boot");

    assert_eq!(engine.fingerprint(), KNOWN_FINGERPRINT);
    assert_eq!(
        engine.worker_count(),
        32,
        "the engine preallocates 32 pthreads"
    );

    // The catalogue is deserialised into the `data` schema, so queries must be
    // schema-qualified: unqualified `cards` is the user's empty collection.
    let rows = engine.query("SELECT count(*) FROM data.cards").unwrap();
    let count: i64 = rows[0][0].parse().unwrap();
    assert!(
        count > 100_000,
        "expected a populated catalogue, got {count} cards"
    );

    let lotus = engine
        .query(
            "SELECT c._id FROM data.cards c JOIN data.names n ON n._id = c.name \
             WHERE n.name = 'Black Lotus' LIMIT 1",
        )
        .unwrap();
    let id: i64 = lotus[0][0].parse().unwrap();
    assert_eq!(engine.card_by_id(id).unwrap().unwrap().name, "Black Lotus");

    let frame = root().join(".fixtures/lotus-frame.jpg");
    let Some((data, width, height)) = frame.exists().then(|| to_rgba(&frame)).flatten() else {
        skip::skipped("no lotus frame; run .fixtures/fetch-cards.sh inside `nix develop`");
        return;
    };
    let image = Image {
        data: &data,
        width,
        height,
    };

    let quad = engine
        .locate(&image)
        .unwrap()
        .expect("locate() should find the card quad");
    assert_eq!(quad.len(), 4);

    let found = engine.recognize(&image).unwrap();
    assert!(!found.is_empty(), "recognize() found nothing");
    assert_eq!(found[0].name, "Black Lotus");
    assert_eq!(found[0].number, "232");

    engine.close();
    let err = engine.query("SELECT 1").unwrap_err().to_string();
    assert!(err.contains("engine is closed"), "{err}");
}

/// FINDINGS.md §13 measured 6/6 on card name and 4/6 on exact printing through
/// the Node harness. The sandbox must not move those numbers - if it does, the
/// port changed the engine's behaviour rather than just its host.
#[test]
fn accuracy_matches_the_node_harness() {
    if !ready() {
        return;
    }
    let cases = [
        ("lotus", "Black Lotus", "Limited Edition Alpha"),
        ("counterspell", "Counterspell", "Limited Edition Alpha"),
        ("llanowar", "Llanowar Elves", "Core Set 2019"),
        ("shock", "Shock", "Core Set 2021"),
        ("swords", "Swords to Plowshares", "Limited Edition Alpha"),
        ("thoughtseize", "Thoughtseize", "Theros"),
    ];
    let fixtures: Vec<_> = cases
        .iter()
        .filter_map(|(slug, name, edition)| {
            let file = root().join(format!(".fixtures/{slug}-frame.jpg"));
            to_rgba(&file).map(|img| (*name, *edition, img))
        })
        .collect();
    if fixtures.len() != cases.len() {
        skip::skipped(
            "not all six frames decoded; run .fixtures/fetch-cards.sh inside `nix develop`",
        );
        return;
    }

    let mut engine = Engine::open(EngineConfig::default()).expect("engine should boot");

    let (mut names, mut printings) = (0, 0);
    for (want_name, want_edition, (data, width, height)) in &fixtures {
        let found = engine
            .recognize(&Image {
                data,
                width: *width,
                height: *height,
            })
            .unwrap();
        let Some(card) = found.first() else {
            panic!("{want_name}: nothing recognised");
        };
        if card.name == *want_name {
            names += 1;
        }
        let edition = engine
            .card_by_id(card.data_id)
            .unwrap()
            .map(|c| c.edition)
            .unwrap_or_default();
        if card.name == *want_name && edition == *want_edition {
            printings += 1;
        }
        println!("  {:<22} -> {} / {edition}", want_name, card.name);
    }
    engine.close();

    println!(
        "card name {names}/{}, exact printing {printings}/{}",
        fixtures.len(),
        fixtures.len()
    );
    assert_eq!(names, 6, "card-name accuracy regressed");
    // 4/6 is the engine's own ceiling, not the host's: it ships no OCR, never
    // reads the collector number, and cannot separate same-art reprints. The
    // two misses are Llanowar Elves (Dominaria, not M19) and Swords to
    // Plowshares (Foreign Black Border, not Alpha) - both same-art reprints.
    assert_eq!(
        printings, 4,
        "printing accuracy moved - the port changed behaviour"
    );
}
