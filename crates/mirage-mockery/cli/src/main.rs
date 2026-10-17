//! `mockery render`, `mockery sheet` and `mockery check`: custom cards from
//! `.card.toml` files, and `mockery symbols` to fetch the mana symbols once.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chip_scryfall::index::{Index, IndexFile};
use facet::Facet;
use figue::{self as args, DriverError};

use mockery_core::card::Card;
use mockery_core::render::{self, Layout, Paper, Sheet};
use mockery_core::symbols::{self, Symbols};
use mockery_core::{read, real};

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
        /// Card index, as built by `gauntlet sync` (default: the synced one).
        #[facet(args::named, default)]
        index: Option<PathBuf>,
        /// Symbol directory (default: the cache `mockery symbols` fills).
        #[facet(args::named, default)]
        symbols: Option<PathBuf>,
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
        #[facet(args::named, default)]
        index: Option<PathBuf>,
        #[facet(args::named, default)]
        symbols: Option<PathBuf>,
    },
    /// Check cards without rendering: not a real card, and what Insight reads.
    Check {
        #[facet(args::positional)]
        cards: Vec<PathBuf>,
        #[facet(args::named, default)]
        index: Option<PathBuf>,
    },
    /// Fetch Scryfall's mana symbols into the cache.
    Symbols {
        #[facet(args::named, default)]
        symbols: Option<PathBuf>,
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

/// Every card read, refused if real, and read by Insight, before any render.
fn load(paths: &[PathBuf], index: Option<PathBuf>) -> Result<Vec<Card>> {
    if paths.is_empty() {
        bail!("no cards given");
    }
    let index_path = index.unwrap_or_else(Index::default_path);
    let index = IndexFile::open(&index_path).with_context(|| {
        format!(
            "Mirage Mockery checks every card against the card index so it never prints a real one, \
             and there is none at {}: run `gauntlet sync`, or pass --index decks/index.jsonl",
            index_path.display()
        )
    })?;
    let cards = paths
        .iter()
        .map(|p| Card::open(p))
        .collect::<Result<Vec<_>>>()?;
    real::refuse_real(&cards, &index)?;
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

fn symbols_at(dir: Option<PathBuf>) -> Result<Symbols> {
    Symbols::open(&dir.unwrap_or_else(Symbols::default_dir))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    eprintln!("wrote {}", path.display());
    Ok(())
}

fn main() -> Result<()> {
    match parse_args().command {
        Command::Render {
            cards,
            out,
            ppi,
            index,
            symbols,
        } => {
            let loaded = load(&cards, index)?;
            let symbols = symbols_at(symbols)?;
            let out = out.unwrap_or_else(|| cards[0].with_extension("").with_extension("png"));
            let doc = render::compile(&loaded, &symbols, Layout::Cards)?;
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
            index,
            symbols,
        } => {
            let paper = match paper {
                PaperArg::A4 => Paper::A4,
                PaperArg::A3 => Paper::A3,
                PaperArg::Letter => Paper::Letter,
                PaperArg::Legal => Paper::Legal,
            };
            let sheet = Sheet::new(paper, gap, margin, cut_marks)?;
            let loaded = load(&cards, index)?;
            let symbols = symbols_at(symbols)?;
            let doc = render::compile(&loaded, &symbols, Layout::Sheet(sheet))?;
            write(&out, &render::pdf(&doc)?)
        }
        Command::Check { cards, index } => load(&cards, index).map(drop),
        Command::Symbols { symbols } => {
            let dir = symbols.unwrap_or_else(Symbols::default_dir);
            let n = fetch_symbols(&dir)?;
            eprintln!("{n} symbols in {}", dir.display());
            Ok(())
        }
    }
}

#[derive(Facet)]
struct Symbology {
    data: Vec<Symbol>,
}

#[derive(Facet)]
struct Symbol {
    symbol: String,
    #[facet(default)]
    svg_uri: Option<String>,
}

/// Every symbol Scryfall has an SVG for, written into `dir`.
fn fetch_symbols(dir: &Path) -> Result<usize> {
    let agent = ureq::Agent::new_with_defaults();
    let get = |url: &str| -> Result<String> {
        // Scryfall asks for both headers on every request.
        Ok(agent
            .get(url)
            .header("User-Agent", "mirage-mockery/0.1")
            .header("Accept", "application/json;q=0.9,*/*;q=0.8")
            .call()
            .with_context(|| format!("GET {url}"))?
            .body_mut()
            .read_to_string()?)
    };
    let list: Symbology = facet_json::from_str(&get("https://api.scryfall.com/symbology")?)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::create_dir_all(dir)?;
    let mut n = 0;
    for s in list.data {
        let Some(uri) = s.svg_uri else { continue };
        let code = s.symbol.trim_start_matches('{').trim_end_matches('}');
        std::fs::write(dir.join(symbols::file_name(code)), get(&uri)?)?;
        n += 1;
        // Scryfall asks for 50–100ms between requests.
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    Ok(n)
}
