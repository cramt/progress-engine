//! What issue #65's GPU spike needs from a prepared run: every class's
//! narrowed grouping, schedule and questions, built exactly as [`crate::answer`]
//! builds them, so the kernel walks what the CPU engine walks and nothing
//! else. Compiled only with the `gpu-spike` feature.

use std::path::Path;

use anyhow::{Context, Result};
use gauntlet_criteria::{Answering, Grouping, Reading, Schedule};

use crate::{prepare, Library};

/// One class, ready to walk.
pub struct Class {
    pub grouping: Grouping,
    pub schedule: Schedule,
    pub answering: Answering,
    pub reading: Reading,
    pub turns: Vec<usize>,
    /// Whether the class prices mana, which no count-only walk does.
    pub priced: bool,
}

pub struct Prepared {
    pub criteria: gauntlet_toml::Criteria,
    pub classes: Vec<Class>,
}

pub fn prepare(
    deck: &Path,
    criteria_path: &Path,
    on_the_draw: bool,
    index_path: Option<&Path>,
) -> Result<Prepared> {
    let library = Library::load(deck, index_path)?;
    let source = std::fs::read_to_string(criteria_path)
        .with_context(|| format!("reading criteria {}", criteria_path.display()))?;
    let origin = criteria_path.display().to_string();
    let mut criteria = gauntlet_toml::Criteria::parse(&source, &origin)?;
    let run = prepare::prepare(&library, &mut criteria, &origin, on_the_draw).run?;
    let classes = run
        .classes
        .iter()
        .map(|class| {
            Ok(Class {
                grouping: class.grouping(&run.grouping),
                schedule: class.schedule(&run.schedule, &run.grouping),
                answering: class
                    .answering(run.plan)
                    .context("a class named a question this file does not hold")?,
                reading: class.reading(),
                turns: class.turns().to_vec(),
                priced: class.pips().is_some(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Prepared { criteria, classes })
}
