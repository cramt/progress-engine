//! Bring the engine up inside the sandbox and run a few catalogue queries.
//!
//!     cargo run --example query

use std::sync::Arc;
use std::time::Instant;

use gitaxian_probe_engine::{Engine, EngineConfig};

fn main() -> anyhow::Result<()> {
    let started = Instant::now();
    let mut engine = Engine::open(EngineConfig {
        on_progress: Some(Arc::new(|p: gitaxian_probe_engine::Progress| {
            let percent = if p.percent > 0 {
                format!("{}% ", p.percent)
            } else {
                String::new()
            };
            println!("  [{}] {percent}{}", p.stage, p.message);
        })),
        ..Default::default()
    })?;
    println!(
        "\nready in {:.1}s  (version {}, build {})\n",
        started.elapsed().as_secs_f64(),
        engine.version(),
        engine.fingerprint()
    );

    for (label, sql) in [
        ("cards", "SELECT count(*) FROM data.cards"),
        ("names", "SELECT count(*) FROM data.names"),
        ("editions", "SELECT count(*) FROM data.editions"),
    ] {
        let rows = engine.query(sql)?;
        println!("catalogue {label:9} {}", rows[0][0]);
    }

    let rows = engine.query(
        "SELECT n.name, e.name, c.number, c.rarity
         FROM data.cards c
         JOIN data.names n ON n._id = c.name
         JOIN data.editions e ON e._id = c.edition
         WHERE n.name = 'Black Lotus'
         ORDER BY e.release_date",
    )?;
    println!("\nBlack Lotus printings:");
    for row in &rows {
        let cell = |i: usize| row[i].clone();
        println!("  {:<24} #{:<5} {}", cell(1), cell(2), cell(3));
    }

    engine.close();
    Ok(())
}
