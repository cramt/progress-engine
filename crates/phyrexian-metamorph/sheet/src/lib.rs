//! Card images in, a print-and-cut PDF of proxies out.
//!
//! This crate knows nothing about Magic beyond the size of a card: it lays
//! JPEGs out at 63×88 mm in a grid on the page and draws where to cut. Which
//! cards, in which printing, and how many is the caller's business, so a deck,
//! a wanted list or a typed list of printings all end here the same way. It
//! does no I/O, so it builds for the browser as it is.

use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};
use thiserror::Error;

/// A Magic card's trim size. Every printing Scryfall sells as a card is this,
/// whatever the image's own pixel size.
pub const CARD_WIDTH_MM: f32 = 63.0;
pub const CARD_HEIGHT_MM: f32 = 88.0;

/// How far a cut mark stays from the card's corner, so the blade never has to
/// split a mark from the card.
const MARK_OFFSET_MM: f32 = 1.0;
const MARK_LENGTH_MM: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    A4,
    A3,
    Letter,
    Legal,
}

impl Paper {
    /// Portrait, in millimetres.
    pub fn size_mm(self) -> (f32, f32) {
        match self {
            Paper::A4 => (210.0, 297.0),
            Paper::A3 => (297.0, 420.0),
            Paper::Letter => (215.9, 279.4),
            Paper::Legal => (215.9, 355.6),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub paper: Paper,
    /// Space between neighbouring cards. With none, one cut separates two
    /// cards; with some, each card is cut on its own lines.
    pub gap_mm: f32,
    /// The least space left at the paper's edge, which most printers cannot
    /// print into. The grid is centred inside it.
    pub margin_mm: f32,
    pub cut_marks: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            paper: Paper::A4,
            gap_mm: 0.0,
            margin_mm: 5.0,
            cut_marks: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    page: (f32, f32),
    cols: usize,
    rows: usize,
    /// The grid's top-left corner, from the page's top-left, in mm.
    left: f32,
    top: f32,
    gap: f32,
}

impl Layout {
    fn grid(&self) -> Result<Grid, SheetError> {
        if !(self.gap_mm >= 0.0 && self.margin_mm >= 0.0) {
            return Err(SheetError::Negative {
                gap_mm: self.gap_mm,
                margin_mm: self.margin_mm,
            });
        }
        let (width, height) = self.paper.size_mm();
        let fit = |room: f32, card: f32| {
            ((room - 2.0 * self.margin_mm + self.gap_mm) / (card + self.gap_mm)).floor() as usize
        };
        let (cols, rows) = (fit(width, CARD_WIDTH_MM), fit(height, CARD_HEIGHT_MM));
        if cols == 0 || rows == 0 {
            return Err(SheetError::NoRoom(*self));
        }
        let span = |n: usize, card: f32| n as f32 * card + (n - 1) as f32 * self.gap_mm;
        Ok(Grid {
            page: (width, height),
            cols,
            rows,
            left: (width - span(cols, CARD_WIDTH_MM)) / 2.0,
            top: (height - span(rows, CARD_HEIGHT_MM)) / 2.0,
            gap: self.gap_mm,
        })
    }

    /// How many cards one page holds.
    pub fn per_page(&self) -> Result<usize, SheetError> {
        self.grid().map(|g| g.cols * g.rows)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Gray,
    Rgb,
}

/// A baseline or progressive JPEG, which PDF embeds as it is: the bytes from
/// Scryfall are the bytes in the file, with nothing decoded or re-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jpeg {
    data: Vec<u8>,
    width: u32,
    height: u32,
    color: Color,
}

impl Jpeg {
    /// Reads the frame header for the size and colour model PDF needs, and
    /// refuses what it cannot embed faithfully.
    pub fn parse(data: Vec<u8>) -> Result<Jpeg, SheetError> {
        let (width, height, components) = frame(&data).ok_or(SheetError::NotJpeg)?;
        let color = match components {
            1 => Color::Gray,
            3 => Color::Rgb,
            // Adobe's CMYK JPEGs store inverted ink, and a viewer that guesses
            // wrong prints a negative. No Scryfall image is CMYK.
            n => return Err(SheetError::Components(n)),
        };
        Ok(Jpeg {
            data,
            width,
            height,
            color,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Width, height and component count from the first start-of-frame marker.
fn frame(data: &[u8]) -> Option<(u32, u32, u8)> {
    if data.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut at = 2;
    loop {
        if *data.get(at)? != 0xFF {
            return None;
        }
        let marker = *data.get(at + 1)?;
        match marker {
            // Fill bytes before a marker.
            0xFF => at += 1,
            // Markers with no length: TEM and the restart markers.
            0x01 | 0xD0..=0xD7 => at += 2,
            // Start of scan, or end of image, before any frame.
            0xDA | 0xD9 => return None,
            _ => {
                let len = u16::from_be_bytes([*data.get(at + 2)?, *data.get(at + 3)?]) as usize;
                // SOF0-SOF15, less DHT (C4), JPG (C8) and DAC (CC).
                if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                    let seg = data.get(at + 4..at + 2 + len)?;
                    let height = u16::from_be_bytes([seg[1], seg[2]]) as u32;
                    let width = u16::from_be_bytes([seg[3], seg[4]]) as u32;
                    return Some((width, height, *seg.get(5)?));
                }
                at += 2 + len;
            }
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum SheetError {
    #[error("not a JPEG, or one with no frame header before its image data")]
    NotJpeg,
    #[error("a JPEG with {0} colour components; only greyscale and RGB embed faithfully")]
    Components(u8),
    #[error("gap {gap_mm} mm and margin {margin_mm} mm: neither can be negative")]
    Negative { gap_mm: f32, margin_mm: f32 },
    #[error("{:?} paper has no room for one 63×88 mm card inside a {} mm margin", .0.paper, .0.margin_mm)]
    NoRoom(Layout),
}

fn pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}

/// The PDF of `cards`, in order, row by row and page by page. A card given
/// twice by the same reference is embedded once, so four basics cost one
/// image.
pub fn pdf(layout: &Layout, cards: &[&Jpeg]) -> Result<Vec<u8>, SheetError> {
    let grid = layout.grid()?;
    let per_page = grid.cols * grid.rows;

    // Each distinct image once, and which of them each card is.
    let mut images: Vec<&Jpeg> = Vec::new();
    let which: Vec<usize> = cards
        .iter()
        .map(
            |&jpeg| match images.iter().position(|j| std::ptr::eq(*j, jpeg)) {
                Some(i) => i,
                None => {
                    images.push(jpeg);
                    images.len() - 1
                }
            },
        )
        .collect();

    let mut next = Ref::new(1);
    let mut alloc = || next.bump();
    let catalog = alloc();
    let tree = alloc();
    let image_refs: Vec<Ref> = images.iter().map(|_| alloc()).collect();

    let mut pdf = Pdf::new();
    let mut pages = Vec::new();
    // An empty list is still a document: one blank page, rather than a file
    // no viewer opens.
    let chunks: Vec<&[usize]> = if which.is_empty() {
        vec![&[]]
    } else {
        which.chunks(per_page).collect()
    };
    for chunk in chunks {
        let page = alloc();
        let contents = alloc();
        let mut content = Content::new();
        let mut used = Vec::new();
        for (slot, &i) in chunk.iter().enumerate() {
            if !used.contains(&i) {
                used.push(i);
            }
            let (col, row) = (slot % grid.cols, slot / grid.cols);
            let x = grid.left + col as f32 * (CARD_WIDTH_MM + grid.gap);
            let top = grid.top + row as f32 * (CARD_HEIGHT_MM + grid.gap);
            let y = grid.page.1 - top - CARD_HEIGHT_MM;
            content.save_state();
            content.transform([
                pt(CARD_WIDTH_MM),
                0.0,
                0.0,
                pt(CARD_HEIGHT_MM),
                pt(x),
                pt(y),
            ]);
            content.x_object(Name(format!("Im{i}").as_bytes()));
            content.restore_state();
        }
        if layout.cut_marks {
            cut_marks(&mut content, &grid);
        }
        pdf.stream(contents, &content.finish());

        let mut p = pdf.page(page);
        p.media_box(Rect::new(0.0, 0.0, pt(grid.page.0), pt(grid.page.1)));
        p.parent(tree);
        p.contents(contents);
        let mut resources = p.resources();
        let mut objects = resources.x_objects();
        for i in used {
            objects.pair(Name(format!("Im{i}").as_bytes()), image_refs[i]);
        }
        objects.finish();
        resources.finish();
        p.finish();
        pages.push(page);
    }

    for (jpeg, &id) in images.iter().zip(&image_refs) {
        let mut image = pdf.image_xobject(id, &jpeg.data);
        image.filter(Filter::DctDecode);
        image.width(jpeg.width as i32);
        image.height(jpeg.height as i32);
        match jpeg.color {
            Color::Gray => image.color_space().device_gray(),
            Color::Rgb => image.color_space().device_rgb(),
        };
        image.bits_per_component(8);
        image.finish();
    }

    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).count(pages.len() as i32).kids(pages);
    Ok(pdf.finish())
}

/// A short line in the margin at every card edge, on all four sides of the
/// grid, so a ruler laid across two marks cuts along the card. The marks stay
/// outside the grid: a mark between two cards would end up printed on one.
fn cut_marks(content: &mut Content, grid: &Grid) {
    let (page_w, page_h) = grid.page;
    let step_x = CARD_WIDTH_MM + grid.gap;
    let step_y = CARD_HEIGHT_MM + grid.gap;
    let right = grid.left + grid.cols as f32 * step_x - grid.gap;
    let bottom = grid.top + grid.rows as f32 * step_y - grid.gap;
    let xs = (0..grid.cols).flat_map(|c| {
        let x = grid.left + c as f32 * step_x;
        [x, x + CARD_WIDTH_MM]
    });
    let ys = (0..grid.rows).flat_map(|r| {
        let y = grid.top + r as f32 * step_y;
        [y, y + CARD_HEIGHT_MM]
    });
    // From the page's top-left in mm to PDF points from its bottom-left.
    let mut line = |(x1, y1): (f32, f32), (x2, y2): (f32, f32)| {
        content.move_to(pt(x1), pt(page_h - y1));
        content.line_to(pt(x2), pt(page_h - y2));
    };
    let reach = |from: f32, toward: f32| {
        let start = from + MARK_OFFSET_MM * toward;
        let end = start + MARK_LENGTH_MM * toward;
        (start, end)
    };
    for x in xs {
        let (a, b) = reach(grid.top, -1.0);
        line((x, a), (x, b.max(0.0)));
        let (a, b) = reach(bottom, 1.0);
        line((x, a), (x, b.min(page_h)));
    }
    for y in ys {
        let (a, b) = reach(grid.left, -1.0);
        line((a, y), (b.max(0.0), y));
        let (a, b) = reach(right, 1.0);
        line((a, y), (b.min(page_w), y));
    }
    content.set_line_width(0.25);
    content.stroke();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest JPEG `frame` reads: SOI, an APP0 to skip, then SOF0.
    fn jpeg(width: u16, height: u16, components: u8) -> Vec<u8> {
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        data.extend([0xFF, 0xC0, 0x00, 0x08, 0x08]);
        data.extend(height.to_be_bytes());
        data.extend(width.to_be_bytes());
        data.push(components);
        data.extend([0xFF, 0xD9]);
        data
    }

    #[test]
    fn a_frame_header_gives_the_size() {
        let j = Jpeg::parse(jpeg(672, 936, 3)).unwrap();
        assert_eq!(j.size(), (672, 936));
        assert_eq!(j.color, Color::Rgb);
    }

    #[test]
    fn cmyk_and_non_jpegs_are_refused() {
        assert_eq!(Jpeg::parse(jpeg(10, 10, 4)), Err(SheetError::Components(4)));
        assert_eq!(
            Jpeg::parse(b"\x89PNG\r\n".to_vec()),
            Err(SheetError::NotJpeg)
        );
        assert_eq!(
            Jpeg::parse(vec![0xFF, 0xD8, 0xFF, 0xDA]),
            Err(SheetError::NotJpeg)
        );
    }

    #[test]
    fn nine_cards_fit_a4_and_letter() {
        for paper in [Paper::A4, Paper::Letter] {
            let layout = Layout {
                paper,
                ..Layout::default()
            };
            assert_eq!(layout.per_page(), Ok(9), "{paper:?}");
        }
        let a3 = Layout {
            paper: Paper::A3,
            ..Layout::default()
        };
        assert_eq!(a3.per_page(), Ok(4 * 4));
    }

    #[test]
    fn a_gap_can_push_a_row_off_the_page() {
        // Letter is 279.4 mm tall: three rows of 88 and a 5 mm margin each
        // side leave 5.4 mm for two gaps.
        let layout = |gap_mm| Layout {
            paper: Paper::Letter,
            gap_mm,
            ..Layout::default()
        };
        assert_eq!(layout(2.7).per_page(), Ok(9));
        assert_eq!(layout(2.8).per_page(), Ok(6));
    }

    #[test]
    fn a_layout_with_no_room_is_refused() {
        let layout = Layout {
            margin_mm: 80.0,
            ..Layout::default()
        };
        assert!(matches!(layout.per_page(), Err(SheetError::NoRoom(_))));
        let layout = Layout {
            gap_mm: -1.0,
            ..Layout::default()
        };
        assert!(matches!(
            layout.per_page(),
            Err(SheetError::Negative { .. })
        ));
        let layout = Layout {
            gap_mm: f32::NAN,
            ..Layout::default()
        };
        assert!(matches!(
            layout.per_page(),
            Err(SheetError::Negative { .. })
        ));
    }

    fn count(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }

    #[test]
    fn ten_cards_are_two_pages_and_a_repeat_is_embedded_once() {
        let island = Jpeg::parse(jpeg(672, 936, 3)).unwrap();
        let bolt = Jpeg::parse(jpeg(672, 936, 1)).unwrap();
        let mut cards = vec![&island; 9];
        cards.push(&bolt);
        let out = pdf(&Layout::default(), &cards).unwrap();
        assert!(out.starts_with(b"%PDF-"));
        assert_eq!(count(&out, b"/Type /Page\n"), 2);
        assert_eq!(count(&out, b"/Subtype /Image"), 2);
        assert_eq!(count(&out, b"/DeviceGray"), 1);
    }
}
