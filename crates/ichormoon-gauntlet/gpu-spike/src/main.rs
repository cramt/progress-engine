//! `gpu-spike <deck> <criteria> [--index PATH] [--draw] [--backend cuda|cpu|all]
//! [--f32] [--no-engine] [--prefixes N]`
//!
//! Prepares the criteria file exactly as `gauntlet test` does, and for every
//! class that is a count-only walk, answers it three ways: the exact engine,
//! and the kernel on each CubeCL backend. Prints the answers side by side to
//! the engine's six printed places, and how long each took.

mod kernel;
mod lower;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use chip_stats::KahanSum;
use cubecl::prelude::*;

use lower::Lowered;

struct Args {
    deck: PathBuf,
    criteria: PathBuf,
    index: Option<PathBuf>,
    draw: bool,
    backends: Vec<String>,
    f32: bool,
    engine: bool,
    prefixes: usize,
}

fn args() -> Result<Args> {
    let mut it = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut a = Args {
        deck: PathBuf::new(),
        criteria: PathBuf::new(),
        index: None,
        draw: false,
        backends: vec!["cuda".into(), "cpu".into()],
        f32: false,
        engine: true,
        prefixes: 20_000,
    };
    while let Some(x) = it.next() {
        match x.as_str() {
            "--index" => a.index = Some(it.next().context("--index needs a path")?.into()),
            "--draw" => a.draw = true,
            "--f32" => a.f32 = true,
            "--no-engine" => a.engine = false,
            "--backend" => {
                a.backends = match it.next().context("--backend needs a name")?.as_str() {
                    "all" => vec!["cuda".into(), "cpu".into()],
                    b => vec![b.into()],
                }
            }
            "--prefixes" => a.prefixes = it.next().context("--prefixes needs a count")?.parse()?,
            _ => positional.push(x),
        }
    }
    let [deck, criteria] = <[String; 2]>::try_from(positional)
        .map_err(|_| anyhow::anyhow!("usage: gpu-spike <deck> <criteria> [options]"))?;
    a.deck = deck.into();
    a.criteria = criteria.into();
    Ok(a)
}

/// One backend's answer to one class.
struct Answer {
    probabilities: Vec<f64>,
    mass: f64,
    paths: f64,
    /// The first launch, which compiles the kernel.
    cold: Duration,
    /// A second launch of the same kernel, compiled already.
    warm: Duration,
}

fn main() -> Result<()> {
    let a = args()?;
    let mut prepared =
        gauntlet_cli::spike::prepare(&a.deck, &a.criteria, a.draw, a.index.as_deref())?;
    let names: Vec<String> = prepared
        .criteria
        .criteria()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    let threads = gauntlet_criteria::threads();
    println!(
        "{} on the {}, {} classes, engine on {threads} threads",
        a.criteria.display(),
        if a.draw { "draw" } else { "play" },
        prepared.classes.len(),
    );

    for (n, class) in prepared.classes.iter().enumerate() {
        let lowered = match lower::lower(class, &prepared.criteria) {
            Ok(l) => l,
            Err(why) => {
                println!("\nclass {n}: skipped, {why}");
                continue;
            }
        };
        let width = gauntlet_criteria::compositions(class.grouping.dealt(), class.schedule.gaps());
        println!(
            "\nclass {n}: {} groups, gaps {:?}, {width} compositions{}",
            class.grouping.dealt(),
            lowered.gaps,
            if lowered.land_drops {
                ", reads land drops"
            } else {
                ""
            },
        );

        let engine = if a.engine {
            let start = Instant::now();
            let out = gauntlet_criteria::run_answering(
                &class.grouping,
                &class.schedule,
                &class.answering,
                &mut prepared.criteria,
            )
            .map_err(|e| anyhow::anyhow!("the engine refused: {e}"))?;
            let took = start.elapsed();
            Some((
                out.probabilities
                    .iter()
                    .map(|p| p.get())
                    .collect::<Vec<_>>(),
                took,
            ))
        } else {
            None
        };

        let prefix = Prefixes::deal(&lowered, a.prefixes);
        println!(
            "  split after checkpoint {} into {} prefixes, {:.1?} on the host",
            prefix.known,
            prefix.p.len(),
            prefix.took
        );

        let mut answers = Vec::new();
        for backend in &a.backends {
            let answer = match (backend.as_str(), a.f32) {
                #[cfg(feature = "cuda")]
                ("cuda", false) => run::<cubecl::cuda::CudaRuntime, f64>(&lowered, &prefix),
                #[cfg(feature = "cuda")]
                ("cuda", true) => run::<cubecl::cuda::CudaRuntime, f32>(&lowered, &prefix),
                #[cfg(feature = "cpu")]
                ("cpu", false) => run::<cubecl::cpu::CpuRuntime, f64>(&lowered, &prefix),
                #[cfg(feature = "cpu")]
                ("cpu", true) => run::<cubecl::cpu::CpuRuntime, f32>(&lowered, &prefix),
                (b, _) => bail!("backend {b} is not built in"),
            };
            answers.push((backend.clone(), answer));
        }

        let paths = answers.first().map_or(0.0, |(_, x)| x.paths);
        if let Some((_, took)) = &engine {
            println!(
                "  engine        {:>9.3?}  {:>7.1} M paths/s",
                took,
                paths / took.as_secs_f64() / 1e6
            );
        }
        for (b, x) in &answers {
            println!(
                "  {b:<5} {}  cold {:>9.3?}  warm {:>9.3?}  {:>7.1} M paths/s  mass-1 {:+.1e}",
                if a.f32 { "f32" } else { "f64" },
                x.cold,
                x.warm,
                x.paths / x.warm.as_secs_f64() / 1e6,
                x.mass - 1.0,
            );
        }
        println!("  {paths} paths walked");
        for (k, &i) in lowered.criteria.iter().enumerate() {
            let mut line = String::new();
            let mut worst = 0f64;
            if let Some((e, _)) = &engine {
                line += &format!(" engine {:.6}", e[k]);
            }
            for (b, x) in &answers {
                line += &format!("  {b} {:.6}", x.probabilities[k]);
                if let Some((e, _)) = &engine {
                    worst = worst.max((x.probabilities[k] - e[k]).abs());
                }
            }
            let agree = match &engine {
                Some((e, _)) => {
                    let same = answers.iter().all(|(_, x)| {
                        format!("{:.6}", x.probabilities[k]) == format!("{:.6}", e[k])
                    });
                    format!(
                        " {} (|diff| {worst:.1e})",
                        if same { "same" } else { "DIFFERS" }
                    )
                }
                None => String::new(),
            };
            println!("  {line}{agree}  {}", names[i]);
        }
    }
    Ok(())
}

/// The first `known` checkpoints, dealt on the host by the engine's own walk.
struct Prefixes {
    known: usize,
    hist: Vec<u32>,
    p: Vec<f64>,
    took: Duration,
}

impl Prefixes {
    /// Deal as few checkpoints as give at least `want` prefixes, so every
    /// thread has a walk of its own and none is a whole run.
    fn deal(l: &Lowered, want: usize) -> Prefixes {
        let start = Instant::now();
        let mut known = 1;
        while known < l.gaps.len()
            && (gauntlet_criteria::compositions(l.sizes.len(), &l.gaps[..known]) as usize) < want
        {
            known += 1;
        }
        let (mut hist, mut p) = (Vec::new(), Vec::new());
        chip_stats::for_each_checkpoint_path(&l.sizes, &l.gaps[..known], |path, q| {
            for row in path {
                hist.extend_from_slice(row);
            }
            p.push(q);
        });
        Prefixes {
            known,
            hist,
            p,
            took: start.elapsed(),
        }
    }
}

/// `C(n, k)` for every `n` up to the population and `k` up to the largest gap,
/// as the float the kernel multiplies in. Exact integers in f64 at these
/// sizes, where the engine goes through log-gamma.
fn binomials(population: u32, widest: u32) -> Vec<f64> {
    let stride = widest as usize + 1;
    let mut t = vec![0f64; (population as usize + 1) * stride];
    for n in 0..=population as usize {
        t[n * stride] = 1.0;
        for k in 1..stride.min(n + 1) {
            t[n * stride + k] =
                t[(n - 1) * stride + k - 1] + if k < n { t[(n - 1) * stride + k] } else { 0.0 };
        }
    }
    t
}

fn run<R: Runtime, F>(l: &Lowered, prefix: &Prefixes) -> Answer
where
    F: Float + CubeElement + num_traits::NumCast,
{
    let client = R::client(&Default::default());
    let widest = l.gaps.iter().copied().max().unwrap_or(0);
    let population: u32 = l.sizes.iter().sum();
    let binom: Vec<F> = binomials(population, widest)
        .into_iter()
        .map(|x| F::from(x).expect("a binomial fits the float"))
        .collect();
    let p: Vec<F> = prefix.p.iter().map(|&x| F::from(x).unwrap()).collect();
    let clauses: Vec<u32> = l.clauses.iter().flatten().copied().collect();
    let criteria = l.crit_end.len();
    let cols = criteria + kernel::TAIL;

    let launch = || {
        let upload = |x: &[u32]| client.create_from_slice(u32::as_bytes(x));
        let sizes = upload(&l.sizes);
        let gaps = upload(&l.gaps);
        let hist = upload(&prefix.hist);
        let pp = client.create_from_slice(F::as_bytes(&p));
        let bb = client.create_from_slice(F::as_bytes(&binom));
        let cl = upload(&clauses);
        let ce = upload(&l.conj_end);
        let cr = upload(&l.crit_end);
        let n = prefix.p.len();
        let out = client.empty(n * cols * std::mem::size_of::<F>());
        let start = Instant::now();
        let dim = 128u32;
        unsafe {
            kernel::walk::launch_unchecked::<F, R>(
                &client,
                CubeCount::Static((n as u32).div_ceil(dim), 1, 1),
                CubeDim::new_1d(dim),
                ArrayArg::from_raw_parts(sizes, l.sizes.len()),
                ArrayArg::from_raw_parts(gaps, l.gaps.len()),
                ArrayArg::from_raw_parts(hist, prefix.hist.len()),
                ArrayArg::from_raw_parts(pp, n),
                ArrayArg::from_raw_parts(bb, binom.len()),
                ArrayArg::from_raw_parts(cl, clauses.len()),
                ArrayArg::from_raw_parts(ce, l.conj_end.len()),
                ArrayArg::from_raw_parts(cr, criteria),
                ArrayArg::from_raw_parts(out.clone(), n * cols),
                n as u32,
                l.sizes.len(),
                l.gaps.len(),
                prefix.known,
                criteria,
                widest as usize + 1,
            );
        }
        let bytes = client
            .read_one(out)
            .expect("the kernel's output reads back");
        let took = start.elapsed();
        (F::from_bytes(&bytes).to_vec(), took)
    };
    let (_, cold) = launch();
    let (out, warm) = launch();

    // Summed on the host in prefix order, so the answer does not depend on
    // how the device scheduled its threads.
    let mut sums = vec![KahanSum::new(); cols];
    for row in out.chunks(cols) {
        for (s, x) in sums.iter_mut().zip(row) {
            s.add(x.to_f64().unwrap());
        }
    }
    let sums: Vec<f64> = sums.into_iter().map(KahanSum::total).collect();
    Answer {
        probabilities: sums[..criteria].to_vec(),
        mass: sums[criteria],
        paths: sums[criteria + 1],
        cold,
        warm,
    }
}
