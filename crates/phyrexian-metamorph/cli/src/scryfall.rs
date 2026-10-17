//! What a card looks like, from Scryfall: one `POST /cards/collection` per 75
//! cards for the image links, then each image from Scryfall's image host.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chip_decklist::deck::CardRef;
use chip_decklist::identity::same_name;
use facet::Facet;
use metamorph_sheet::Jpeg;

const USER_AGENT: &str = concat!("phyrexian-metamorph/", env!("CARGO_PKG_VERSION"));
const COLLECTION_API: &str = "https://api.scryfall.com/cards/collection";
/// Scryfall's own limit on identifiers per request.
const BATCH: usize = 75;
/// Scryfall asks for 50-100 ms between API requests. Its image host has no
/// such limit, so images are not paced.
const REQUEST_GAP: Duration = Duration::from_millis(100);

/// Which of Scryfall's images goes on the page. All are JPEG, which the sheet
/// embeds as it is; `png` is left out because it would have to be decoded.
#[derive(Facet, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[facet(rename_all = "kebab-case")]
#[repr(u8)]
pub enum Version {
    /// 672×936, about 270 dpi at card size.
    #[default]
    Large,
    /// 488×680, for a smaller file.
    Normal,
    /// 480×680, the card cropped to its border.
    BorderCrop,
}

#[derive(Facet, Debug, Default)]
struct ImageUris {
    #[facet(default)]
    large: Option<String>,
    #[facet(default)]
    normal: Option<String>,
    #[facet(default)]
    border_crop: Option<String>,
}

impl ImageUris {
    fn get(&self, version: Version) -> Option<&str> {
        match version {
            Version::Large => self.large.as_deref(),
            Version::Normal => self.normal.as_deref(),
            Version::BorderCrop => self.border_crop.as_deref(),
        }
    }
}

#[derive(Facet, Debug)]
struct Face {
    #[facet(default)]
    image_uris: Option<ImageUris>,
}

#[derive(Facet, Debug)]
struct Card {
    name: String,
    set: String,
    collector_number: String,
    /// On a card whose faces share one side of the paper: split, flip,
    /// adventure. A card with two sides has it on each face instead.
    #[facet(default)]
    image_uris: Option<ImageUris>,
    #[facet(default)]
    card_faces: Option<Vec<Face>>,
}

#[derive(Facet, Debug)]
struct Answer {
    data: Vec<Card>,
}

/// One printing, as Scryfall answered for a card the caller named.
#[derive(Debug, Clone)]
pub struct Found {
    pub name: String,
    pub set: String,
    pub num: String,
    /// One image link per side of the card, front first.
    pub sides: Vec<String>,
}

fn identifier(card: &CardRef) -> Result<String> {
    let json = |s: &str| facet_json::to_string(&s.to_string()).context("writing a request");
    Ok(match card {
        CardRef::Printing(p) => format!(
            r#"{{"set":{},"collector_number":{}}}"#,
            json(&p.set)?,
            json(&p.num)?
        ),
        CardRef::Name(name) => format!(r#"{{"name":{}}}"#, json(name)?),
    })
}

fn answers(card: &CardRef, found: &Card) -> bool {
    match card {
        CardRef::Printing(p) => {
            p.set.eq_ignore_ascii_case(&found.set) && p.num == found.collector_number
        }
        CardRef::Name(name) => same_name(name, &found.name),
    }
}

/// Each of `cards` as Scryfall knows it, in the order given. A card Scryfall
/// does not know is an error naming every one, rather than a missing proxy
/// nobody notices until the deck is sleeved.
pub fn look_up(cards: &[CardRef], version: Version) -> Result<Vec<Found>> {
    let mut known: Vec<Card> = Vec::new();
    for (i, batch) in cards.chunks(BATCH).enumerate() {
        if i > 0 {
            std::thread::sleep(REQUEST_GAP);
        }
        let ids = batch.iter().map(identifier).collect::<Result<Vec<_>>>()?;
        let body = format!(r#"{{"identifiers":[{}]}}"#, ids.join(","));
        let text = ureq::post(COLLECTION_API)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .content_type("application/json")
            .send(&body)
            .context("asking Scryfall for the cards")?
            .body_mut()
            .read_to_string()
            .context("reading Scryfall's answer")?;
        let answer: Answer = facet_json::from_str(&text).context("parsing Scryfall's answer")?;
        known.extend(answer.data);
    }

    let mut missing = Vec::new();
    let mut out = Vec::new();
    for card in cards {
        let Some(found) = known.iter().find(|k| answers(card, k)) else {
            missing.push(card.to_string());
            continue;
        };
        let sides: Option<Vec<String>> = match (&found.image_uris, &found.card_faces) {
            (Some(uris), _) => uris.get(version).map(|u| vec![u.to_string()]),
            (None, Some(faces)) => faces
                .iter()
                .map(|f| f.image_uris.as_ref()?.get(version).map(str::to_string))
                .collect(),
            (None, None) => None,
        };
        let Some(sides) = sides.filter(|s| !s.is_empty()) else {
            bail!(
                "Scryfall has no {version:?} image of {} ({card})",
                found.name
            );
        };
        out.push(Found {
            name: found.name.clone(),
            set: found.set.clone(),
            num: found.collector_number.clone(),
            sides,
        });
    }
    if !missing.is_empty() {
        bail!("Scryfall knows no card for: {}", missing.join(", "));
    }
    Ok(out)
}

/// Every image `links` names, each fetched once however often it is named.
pub fn images<'a>(links: impl IntoIterator<Item = &'a str>) -> Result<HashMap<String, Jpeg>> {
    let mut distinct: Vec<&str> = links.into_iter().collect();
    distinct.sort_unstable();
    distinct.dedup();
    let per_worker = distinct.len().div_ceil(IMAGE_WORKERS).max(1);
    std::thread::scope(|scope| {
        let workers: Vec<_> = distinct
            .chunks(per_worker)
            .map(|chunk| scope.spawn(move || chunk.iter().map(|&l| image(l)).collect::<Vec<_>>()))
            .collect();
        let mut images = HashMap::new();
        for worker in workers {
            for result in worker.join().expect("an image worker panicked") {
                let (link, jpeg) = result?;
                images.insert(link, jpeg);
            }
        }
        Ok(images)
    })
}

/// One fetch takes a quarter second, nearly all of it waiting on the round
/// trip, so a deck's hundred images one at a time is most of a run.
const IMAGE_WORKERS: usize = 8;

fn image(link: &str) -> Result<(String, Jpeg)> {
    let bytes = ureq::get(link)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("fetching {link}"))?
        .body_mut()
        .read_to_vec()
        .with_context(|| format!("reading {link}"))?;
    let jpeg = Jpeg::parse(bytes).with_context(|| format!("embedding {link}"))?;
    Ok((link.to_string(), jpeg))
}
