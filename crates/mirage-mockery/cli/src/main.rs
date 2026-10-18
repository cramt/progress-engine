//! `mockery render`, `mockery sheet` and `mockery check`: custom cards from
//! `.card.toml` files.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use facet::Facet;
use figue::{self as args, DriverError};

use mockery_core::card::Card;
use mockery_core::read;
use mockery_core::render::{self, Layout, Paper, Sheet};

#[derive(Facet)]
struct Cli {
    #[facet(args::subcommand)]
    command: Command,
    /// Show help message and exit.
    #[facet(args::named, args::short = 'h', args::help, default)]
    help: bool,
}

#[derive(Facet)]
#[repr(u8)]
enum Command {
    /// Render cards, each a page of its own: a PNG for one card, or a PDF.
    Render {
        /// `.card.toml` files.
        #[facet(args::positional)]
        cards: Vec<PathBuf>,
        /// Where to write; `.png` or `.pdf`. Default: beside the first card, as a PNG.
        #[facet(args::named, args::short = 'o', default)]
        out: Option<PathBuf>,
        /// Pixels per inch for a PNG.
        #[facet(args::named, default = 300)]
        ppi: u32,
    },
    /// As many cards as fit a page, as a PDF to print and cut. The flags are
    /// Phyrexian Metamorph's, so a proxy sheet and a custom one print alike.
    Sheet {
        #[facet(args::positional)]
        cards: Vec<PathBuf>,
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
    },
    /// Check cards without rendering: what Insight reads.
    Check {
        #[facet(args::positional)]
        cards: Vec<PathBuf>,
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

fn parse_args() -> Cli {
    let config = match figue::builder::<Cli>() {
        Ok(builder) => builder
            .cli(|cli| cli.args(std::env::args().skip(1)))
            .help(|help| {
                help.program_name("mockery")
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
        Err(DriverError::Help { text, .. }) => {
            println!("{text}");
            std::process::exit(0);
        }
        Err(other) => {
            eprintln!("{other}");
            std::process::exit(other.exit_code().max(1));
        }
    }
}

/// Every card read, and read by Insight, before any render.
fn load(paths: &[PathBuf]) -> Result<Vec<Card>> {
    if paths.is_empty() {
        bail!("no cards given");
    }
    let cards = paths
        .iter()
        .map(|p| Card::open(p))
        .collect::<Result<Vec<_>>>()?;
    for card in &cards {
        let reading = read::read(card);
        let read_ok = reading.lines - reading.unread.len();
        eprintln!(
            "{}: Insight read {read_ok}/{} lines",
            card.name, reading.lines
        );
        for u in &reading.unread {
            eprintln!("  stopped at {:?} after {:?} in: {}", u.at, u.read, u.line);
        }
    }
    Ok(cards)
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn main() -> Result<()> {
    match parse_args().command {
        Command::Render { cards, out, ppi } => {
            let loaded = load(&cards)?;
            let out = out.unwrap_or_else(|| cards[0].with_extension("").with_extension("png"));
            let doc = render::compile(&loaded, Layout::Cards)?;
            match out.extension().and_then(|e| e.to_str()) {
                Some("pdf") => write(&out, &render::pdf(&doc)?),
                Some("png") if loaded.len() == 1 => write(&out, &render::png(&doc, 0, ppi as f32)?),
                Some("png") => bail!("a PNG holds one card; render several to a .pdf"),
                _ => bail!("{} is neither .png nor .pdf", out.display()),
            }
        }
        Command::Sheet {
            cards,
            out,
            paper,
            gap,
            margin,
            cut_marks,
        } => {
            let paper = match paper {
                PaperArg::A4 => Paper::A4,
                PaperArg::A3 => Paper::A3,
                PaperArg::Letter => Paper::Letter,
                PaperArg::Legal => Paper::Legal,
            };
            let sheet = Sheet::new(paper, gap, margin, cut_marks)?;
            let loaded = load(&cards)?;
            let doc = render::compile(&loaded, Layout::Sheet(sheet))?;
            write(&out, &render::pdf(&doc)?)
        }
        Command::Check { cards } => load(&cards).map(drop),
    }
}
