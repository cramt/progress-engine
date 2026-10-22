//! `metamorph`: a PDF of proxies to print and cut.
//!
//! `printings` is the whole tool: printings in, PDF out. `deck` is that with
//! the list worked out from a `.deck.toml`, less what the caller says to
//! leave out.

mod scryfall;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chip_decklist::collection::Collection;
use chip_decklist::deck::{CardRef, Deck, Printing};
use chip_decklist::identity::Names;
use chip_decklist::stem::DeckStem;
use chip_decklist::supply::supply;
use facet::Facet;
use figue::{self as args, DriverError, Shell};
use metamorph_sheet::{Jpeg, Layout, Paper};

use scryfall::{Found, Version};

/// Proxies of Magic cards, as a PDF to print and cut.
#[derive(Facet)]
struct Cli {
    #[facet(args::subcommand)]
    command: Command,
    #[facet(flatten)]
    _builtins: Builtins,
}

/// figue's `FigueBuiltins`, less the two Gauntlet found to lie (see
/// `gauntlet-cli`'s main.rs).
#[derive(Facet, Default)]
struct Builtins {
    /// Show help message and exit.
    #[facet(args::named, args::short = 'h', args::help, default)]
    help: bool,
    /// Show version and exit.
    #[facet(args::named, args::short = 'V', args::version, default)]
    version: bool,
    /// Generate shell completions.
    #[facet(args::named, args::completions, default)]
    completions: Option<Shell>,
}

#[derive(Facet)]
#[repr(u8)]
enum Command {
    /// Print a list of printings.
    ///
    /// Each is `set/num`, as Scryfall writes it. Name a printing twice for two
    /// copies.
    Printings {
        #[facet(args::positional)]
        printings: Vec<String>,
        #[facet(flatten)]
        sheet: Sheet,
    },
    /// Print a deck's cards.
    ///
    /// Every card of a `.deck.toml` that needs a physical copy: not the
    /// maybeboard, nor what is set aside.
    Deck {
        #[facet(args::positional)]
        deck: PathBuf,
        /// Leave the basic lands out.
        #[facet(args::named, default)]
        no_basics: bool,
        /// Leave out what this `collection.toml` owns, in any printing, as
        /// Curator's Sourcing view counts it.
        #[facet(args::named, default)]
        minus: Option<PathBuf>,
        #[facet(flatten)]
        sheet: Sheet,
    },
}

#[derive(Facet, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
enum PaperArg {
    #[default]
    A4,
    A3,
    Letter,
    Legal,
}

/// Which sides of a card with two get a proxy.
#[derive(Facet, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
enum Sides {
    /// Each side its own proxy, so both can be read.
    #[default]
    Both,
    /// The front only.
    Front,
}

#[derive(Facet)]
struct Sheet {
    /// Where to write the PDF.
    #[facet(args::named, args::short = 'o')]
    out: PathBuf,
    /// Paper size, portrait.
    #[facet(args::named, default)]
    paper: PaperArg,
    /// Millimetres between cards.
    #[facet(args::named, default = 0.0_f32)]
    gap: f32,
    /// Millimetres kept clear at the paper's edge.
    #[facet(args::named, default = 5.0_f32)]
    margin: f32,
    /// Lines in the margin along every card edge.
    #[facet(args::named, default = true)]
    cut_marks: bool,
    /// Scryfall's image to print: large is about 270 dpi at card size.
    #[facet(args::named, default)]
    image: Version,
    /// For a card with two sides: a proxy of each, or of the front.
    #[facet(args::named, default)]
    sides: Sides,
}

/// A card to print, and how many.
struct Proxy {
    found: Found,
    qty: u32,
}

fn main() {
    if let Err(error) = run(parse_args()) {
        eprintln!("metamorph: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Printings { printings, sheet } => {
            let mut counted: Vec<(CardRef, u32)> = Vec::new();
            for text in &printings {
                let card = Printing::parse(text)
                    .map(CardRef::Printing)
                    .with_context(|| format!("{text:?} is not a printing, `set/num`"))?;
                match counted.iter_mut().find(|(c, _)| *c == card) {
                    Some((_, qty)) => *qty += 1,
                    None => counted.push((card, 1)),
                }
            }
            let cards: Vec<CardRef> = counted.iter().map(|(c, _)| c.clone()).collect();
            let found = scryfall::look_up(&cards, sheet.image)?;
            let proxies = found
                .into_iter()
                .zip(&counted)
                .map(|(found, &(_, qty))| Proxy { found, qty })
                .collect();
            print(&sheet, proxies)
        }
        Command::Deck {
            deck,
            no_basics,
            minus,
            sheet,
        } => {
            let path = deck.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&deck).with_context(|| format!("reading {path}"))?;
            let parsed = Deck::parse(&text).with_context(|| format!("reading {path}"))?;
            let (collection, collection_text) = match &minus {
                Some(file) => {
                    let text = std::fs::read_to_string(file)
                        .with_context(|| format!("reading {}", file.display()))?;
                    let parsed = Collection::parse(&text)
                        .with_context(|| format!("reading {}", file.display()))?;
                    (parsed, text)
                }
                None => (Collection::default(), String::new()),
            };

            // Every card the deck names, looked up once, so the names a
            // printing goes by come from Scryfall rather than the comments.
            let mut cards: Vec<CardRef> = Vec::new();
            for c in &parsed.cards {
                if !cards.contains(&c.card) {
                    cards.push(c.card.clone());
                }
            }
            let found = scryfall::look_up(&cards, sheet.image)?;
            let names: Names = cards
                .iter()
                .zip(&found)
                .filter_map(|(card, f)| match card {
                    CardRef::Printing(p) => Some((p.to_string(), f.name.clone())),
                    CardRef::Name(_) => None,
                })
                .collect();

            // A collection's deck place names the deck by its stem, which is
            // the file's name in a checkout of the deck (ADR-0041).
            let stem = DeckStem::of_file(&path);
            let supply = supply(
                stem.as_ref(),
                (&parsed, &text),
                (&collection, &collection_text),
                &[],
                &names,
            );
            let to_print = supply.to_print(no_basics);
            let proxies = to_print
                .lines
                .iter()
                .map(|&(index, qty)| {
                    let card = &parsed.cards[index].card;
                    let i = cards.iter().position(|c| c == card).expect("looked up");
                    Proxy {
                        found: found[i].clone(),
                        qty,
                    }
                })
                .collect();
            // The run's numbers depend on what was left out, so it says.
            if no_basics {
                eprintln!("left out {} basic lands (--no-basics)", to_print.basics);
            }
            if let Some(file) = &minus {
                eprintln!(
                    "left out {} cards {} owns (--minus)",
                    to_print.owned,
                    file.display()
                );
            }
            print(&sheet, proxies)
        }
    }
}

fn print(sheet: &Sheet, proxies: Vec<Proxy>) -> Result<()> {
    let layout = Layout {
        paper: match sheet.paper {
            PaperArg::A4 => Paper::A4,
            PaperArg::A3 => Paper::A3,
            PaperArg::Letter => Paper::Letter,
            PaperArg::Legal => Paper::Legal,
        },
        gap_mm: sheet.gap,
        margin_mm: sheet.margin,
        cut_marks: sheet.cut_marks,
    };
    let per_page = layout.per_page()?;

    let sides = |p: &Proxy| -> Vec<String> {
        match sheet.sides {
            Sides::Both => p.found.sides.clone(),
            Sides::Front => p.found.sides[..1].to_vec(),
        }
    };
    let links: Vec<String> = proxies.iter().flat_map(sides).collect();
    let images = scryfall::images(links.iter().map(String::as_str))?;

    let mut cards: Vec<&Jpeg> = Vec::new();
    for p in &proxies {
        println!(
            "{} {} ({}/{})",
            p.qty, p.found.name, p.found.set, p.found.num
        );
        for _ in 0..p.qty {
            for link in sides(p) {
                cards.push(&images[&link]);
            }
        }
    }
    if cards.is_empty() {
        bail!("nothing left to print");
    }

    let pdf = metamorph_sheet::pdf(&layout, &cards)?;
    std::fs::write(&sheet.out, &pdf).with_context(|| format!("writing {}", sheet.out.display()))?;
    let pages = cards.len().div_ceil(per_page);
    eprintln!(
        "{} proxies of {} cards on {pages} pages of {:?}, {per_page} to a page: {} ({:.1} MB)",
        cards.len(),
        proxies.iter().map(|p| p.qty).sum::<u32>(),
        sheet.paper,
        sheet.out.display(),
        pdf.len() as f64 / 1e6,
    );
    Ok(())
}

/// Parse argv the way `gauntlet` does: help asked for goes to stdout with
/// success, and a usage error goes to stderr with a failure, rather than
/// figue's default of printing help for both and exiting 0.
fn parse_args() -> Cli {
    let asked_for_help = std::env::args().skip(1).any(|a| {
        matches!(a.as_str(), "-h" | "--help" | "-V" | "--version") || a.starts_with("--completions")
    });

    let config = match figue::builder::<Cli>() {
        Ok(builder) => builder
            .cli(|cli| cli.args(std::env::args().skip(1)))
            .help(|help| {
                help.program_name("metamorph")
                    .version(env!("CARGO_PKG_VERSION"))
            })
            .build(),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    match figue::Driver::new(config).run().into_result() {
        Ok(output) => output.get(),
        Err(DriverError::Help { text, suggestion }) => {
            let help = |out: &mut dyn std::io::Write| {
                let _ = writeln!(out, "{text}");
                if let Some(s) = &suggestion {
                    let _ = writeln!(out, "{}", s.render_pretty());
                }
            };
            if asked_for_help {
                help(&mut std::io::stdout());
                std::process::exit(0);
            }
            help(&mut std::io::stderr());
            std::process::exit(2);
        }
        Err(DriverError::Version { text }) => {
            println!("{text}");
            std::process::exit(0);
        }
        Err(DriverError::Completions { script }) => {
            println!("{script}");
            std::process::exit(0);
        }
        Err(other) => {
            eprintln!("{other}");
            std::process::exit(other.exit_code().max(1));
        }
    }
}
