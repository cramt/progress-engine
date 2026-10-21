//! The art a card prints when it has none: an abstract DiceBear image, seeded
//! by the card's name so a card always draws the same one, in the card's own
//! colours so it reads as that card's art and not a hole in the frame.
//!
//! Only DiceBear's CC0 abstract styles are used; their figures and faces would
//! read as a creature the card is not.

use anyhow::{Context, Result};
use dicebear_core::{Avatar, Style};
use serde_json::{json, Value};

use crate::card::{Color, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Blobs,
    Glass,
    Shapes,
    Waves,
}

impl Look {
    const ALL: [Look; 4] = [Look::Blobs, Look::Glass, Look::Shapes, Look::Waves];

    /// Which look a name gets: a sheet of placeholders reads as several
    /// cards rather than one image repeated, and a card keeps its look as it
    /// is edited, since only its name picks it. Glass has one colour, its
    /// background, so a card of several colours never gets it.
    fn of(name: &str, colors: usize) -> Look {
        // FNV-1a: stable across builds and platforms, as std's hasher is not.
        let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        let looks: &[Look] = if colors > 1 {
            &[Look::Blobs, Look::Shapes, Look::Waves]
        } else {
            &Look::ALL
        };
        looks[(hash % looks.len() as u64) as usize]
    }

    fn definition(self) -> &'static str {
        match self {
            Look::Blobs => dicebear_styles::BLOBS,
            Look::Glass => dicebear_styles::GLASS,
            Look::Shapes => dicebear_styles::SHAPES,
            Look::Waves => dicebear_styles::WAVES,
        }
    }

    /// The style's colour options: the darkest colour's dark tone behind and
    /// the others' over it, so every colour of a gold card shows. DiceBear
    /// picks one value of a list per layer, so a list of several colours
    /// would show one of them.
    fn colors(self, tones: &[Tones]) -> Value {
        let (back, rest) = tones.split_first().expect("a card has at least one tone");
        let over = if rest.is_empty() { tones } else { rest };
        let light: Vec<_> = over.iter().map(|t| t.light).collect();
        let mid: Vec<_> = over.iter().map(|t| t.mid).collect();
        match self {
            Look::Blobs => json!({ "backgroundColor": back.deep, "blobColor": light }),
            Look::Waves => json!({ "backgroundColor": back.deep, "waveColor": light }),
            Look::Glass => json!({ "backgroundColor": back.deep }),
            Look::Shapes => json!({
                "backgroundColor": back.deep,
                "shape1Color": mid,
                "shape2Color": back.mid,
                "shape3Color": light,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Tones {
    deep: &'static str,
    mid: &'static str,
    light: &'static str,
}

impl Tones {
    fn of(color: Color) -> Tones {
        let (deep, mid, light) = match color {
            Color::White => ("#c9a94e", "#ecdca6", "#f8e9b4"),
            Color::Blue => ("#1d4f86", "#3f7fb8", "#a9cdea"),
            Color::Black => ("#231d22", "#4d434b", "#9a9192"),
            Color::Red => ("#8f2a1c", "#c8502e", "#f0a787"),
            Color::Green => ("#1f5a35", "#3f8a55", "#9dcb93"),
        };
        Tones { deep, mid, light }
    }

    const COLORLESS: Tones = Tones {
        deep: "#4f5861",
        mid: "#8e98a1",
        light: "#d5dade",
    };
    const LAND: Tones = Tones {
        deep: "#5e4e3b",
        mid: "#9c8467",
        light: "#ddcdb6",
    };
}

/// `#rrggbb`'s relative luminance, near enough to order tones by.
fn luminance(hex: &str) -> u32 {
    let channel = |i: usize| u32::from_str_radix(&hex[i..i + 2], 16).expect("a tone is #rrggbb");
    2126 * channel(1) + 7152 * channel(3) + 722 * channel(5)
}

/// The SVG a face with no art prints, for a face named `name` with frame
/// `frame` and colours `colors`, in WUBRG order.
pub fn art(name: &str, frame: Frame, colors: &[Color]) -> Result<String> {
    let tones: Vec<Tones> = match (frame, colors) {
        (Frame::Land(_), []) => vec![Tones::LAND],
        (_, []) => vec![Tones::COLORLESS],
        (_, colors) => {
            // The darkest colour goes behind: a light colour over a dark one
            // shows, where black over blue all but vanished.
            let mut tones: Vec<Tones> = colors.iter().copied().map(Tones::of).collect();
            tones.sort_by_key(|t| luminance(t.deep));
            tones
        }
    };
    let look = Look::of(name, tones.len());
    let style =
        Style::from_str(look.definition()).with_context(|| format!("DiceBear's {look:?} style"))?;
    let mut options = look.colors(&tones);
    options["seed"] = json!(name);
    let avatar = Avatar::new(&style, options)
        .with_context(|| format!("drawing {name:?}'s placeholder art"))?;
    Ok(avatar.to_svg().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_always_draws_the_same_art() {
        let one = art("Force Push", Frame::Mono(Color::Blue), &[Color::Blue]).unwrap();
        let two = art("Force Push", Frame::Mono(Color::Blue), &[Color::Blue]).unwrap();
        assert_eq!(one, two);
        assert!(one.starts_with("<svg"));
    }

    #[test]
    fn every_look_draws_in_every_frame() {
        let frames = [
            (Frame::Mono(Color::Red), vec![Color::Red]),
            (Frame::Gold, vec![Color::White, Color::Red]),
            (Frame::Colorless, vec![]),
            (Frame::Land(Default::default()), vec![]),
        ];
        // Enough names that each look is drawn.
        let names: Vec<String> = (0..32).map(|i| format!("Card {i}")).collect();
        for look in Look::ALL {
            assert!(
                names.iter().any(|n| Look::of(n, 1) == look),
                "{look:?} never picked"
            );
        }
        for (frame, colors) in &frames {
            for name in &names {
                art(name, *frame, colors).unwrap();
            }
        }
    }

    #[test]
    fn the_art_is_in_the_cards_colours() {
        let svg = art("Card 1", Frame::Mono(Color::Green), &[Color::Green]).unwrap();
        let green = Tones::of(Color::Green);
        assert!(
            [green.deep, green.mid, green.light]
                .iter()
                .any(|hex| svg.to_lowercase().contains(&hex[1..])),
            "{svg}"
        );
    }

    #[test]
    fn a_gold_card_shows_each_of_its_colours() {
        let colors = [Color::Red, Color::White];
        for i in 0..16 {
            let svg = art(&format!("Card {i}"), Frame::Gold, &colors)
                .unwrap()
                .to_lowercase();
            for c in colors {
                let t = Tones::of(c);
                assert!(
                    [t.deep, t.mid, t.light]
                        .iter()
                        .any(|hex| svg.contains(&hex[1..])),
                    "Card {i} lacks {c:?}"
                );
            }
        }
    }
}
