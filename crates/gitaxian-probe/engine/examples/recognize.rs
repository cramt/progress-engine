//! Identify the cards in a still image.
//!
//!     cargo run --example recognize -- .fixtures/lotus-frame.jpg
//!
//! The engine's contract is raw RGBA, so decoding is the caller's problem; this
//! shells out to ImageMagick rather than pulling in an image decoder, the same
//! way the Node harness does.

use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use gitaxian_probe_engine::{Engine, EngineConfig, Image};

fn magick(args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("magick").args(args).output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow::anyhow!("ImageMagick not found - run this inside `nix develop`")
        } else {
            e.into()
        }
    })?;
    if !out.status.success() {
        bail!(
            "magick {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(out.stdout)
}

fn to_rgba(file: &str) -> Result<(Vec<u8>, u32, u32)> {
    let dims = String::from_utf8(magick(&["identify", "-format", "%w %h", file])?)?;
    let (w, h) = dims
        .trim()
        .split_once(' ')
        .context("unexpected identify output")?;
    let data = magick(&[file, "-depth", "8", "RGBA:-"])?;
    Ok((data, w.parse()?, h.parse()?))
}

fn main() -> Result<()> {
    let file = std::env::args()
        .nth(1)
        .unwrap_or_else(|| ".fixtures/lotus-frame.jpg".into());
    let (data, width, height) = to_rgba(&file)?;
    println!("image: {file}  {width}x{height}");

    let mut engine = Engine::open(EngineConfig::default())?;
    println!(
        "engine ready ({}, {:?} model)\n",
        engine.version(),
        engine.tier()
    );

    let image = Image {
        data: &data,
        width,
        height,
    };

    match engine.locate(&image)? {
        Some(quad) => {
            let pts: Vec<_> = quad.iter().map(|p| format!("({},{})", p.x, p.y)).collect();
            println!("locate(): {}", pts.join(" "));
        }
        None => println!("locate(): no card found"),
    }

    let started = Instant::now();
    let cards = engine.recognize(&image)?;
    println!(
        "\nrecognize(): {} card(s) in {}ms",
        cards.len(),
        started.elapsed().as_millis()
    );

    for card in &cards {
        let details = engine.card_by_id(card.data_id)?;
        println!("\n  {}", card.name);
        if let Some(d) = &details {
            println!(
                "    edition   {} (#{}, {})",
                d.edition,
                card.number,
                d.rarity.as_deref().unwrap_or("?")
            );
            println!("    artist    {}", d.artist.as_deref().unwrap_or("?"));
            println!("    scryfall  {}", d.scryfall_id.as_deref().unwrap_or("?"));
        }
        println!(
            "    conf      card {}, set {}",
            card.rec_conf, card.set_conf
        );
        let similar: Vec<_> = card.similar.iter().take(5).map(|i| i.to_string()).collect();
        println!(
            "    dataId    {}   similar: {}",
            card.data_id,
            similar.join(", ")
        );
    }

    engine.close();
    Ok(())
}
