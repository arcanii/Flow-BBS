//! The bulletin typeset for Flow, as the demonstrations' page of type is:
//! the sysop's words set in Noto Serif on a card of cream paper under a red
//! running title, its Japanese, Chinese and Korean in Noto Serif CJK, shaped
//! (kerning, ligatures) and broken into lines by cosmic-text. The card is as
//! wide as the caller's window allows (to a width easy to read) and a whole
//! number of its rows tall, drawn at twice the cells' pixels: each row of
//! cells then shows the same number of its pixels, as Flow on Windows needs
//! (dial draws a picture there a row at a time), and it stays sharp where
//! Flow's pixels are finer than its window says. A caller in any other
//! terminal gets the words, their marks read as text.
//!
//! The bulletin's lines are its paragraphs: `# ` starts a headline, `## ` a
//! subhead, `- ` a point, and a blank line leaves a little room.

use std::path::Path;
use std::sync::{Arc, Mutex};

use cosmic_text::{fontdb, Attrs, Buffer, Color, Family, Fallback, FontSystem, Metrics, Shaping, SwashCache, Weight};
use flow_bbs::printable;
use unicode_script::Script;

use crate::ui::{wrap, BOLD, CYAN, PLAIN};

/// What a line of the bulletin is, by its mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Headline,
    Subhead,
    Point,
    Body,
    Gap,
}

/// The bulletin's lines, each with what its mark makes it and its words.
pub fn blocks(text: &str) -> Vec<(Kind, String)> {
    printable(text)
        .lines()
        .map(|line| {
            let line = line.trim_end();
            let (kind, words) = if line.trim().is_empty() {
                (Kind::Gap, "")
            } else if let Some(w) = line.strip_prefix("# ") {
                (Kind::Headline, w)
            } else if let Some(w) = line.strip_prefix("## ") {
                (Kind::Subhead, w)
            } else if let Some(w) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                (Kind::Point, w)
            } else {
                (Kind::Body, line)
            };
            (kind, words.trim().to_string())
        })
        .collect()
}

/// The bulletin as text, for a terminal that shows no pictures: lines of at
/// most `width` cells, a headline in the board's heading color, a subhead
/// bold, a point after a bullet with its lines under its first.
pub fn text(bulletin: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for (kind, words) in blocks(bulletin) {
        match kind {
            Kind::Gap => lines.push(String::new()),
            Kind::Headline => lines.extend(wrap(&words, width).into_iter().map(|l| format!("{CYAN}{l}{PLAIN}"))),
            Kind::Subhead => lines.extend(wrap(&words, width).into_iter().map(|l| format!("{BOLD}{l}{PLAIN}"))),
            Kind::Point => {
                for (i, l) in wrap(&words, width.saturating_sub(2)).into_iter().enumerate() {
                    lines.push(format!("{}{l}", if i == 0 { "• " } else { "  " }));
                }
            }
            Kind::Body => lines.extend(wrap(&words, width)),
        }
    }
    lines
}

/// The faces the card is set in: a serif for the words, a sans for the
/// running title, and a CJK serif for the letters the first hasn't.
const SERIF: &str = "Noto Serif";
const SANS: &str = "Noto Sans";
const CJK: &str = "Noto Serif CJK JP";

/// The page of type's colors: its paper, the paper's edge, its ink, and its red.
const PAPER: [u8; 3] = [0xf5, 0xf0, 0xe6];
const EDGE: [u8; 3] = [0xd8, 0xcd, 0xb9];
const INK: Color = Color::rgb(0x1f, 0x1c, 0x18);
const RED: Color = Color::rgb(0xb5, 0x32, 0x2a);

/// The card's pixels to a cell's, as its window says them.
const FINE: u32 = 2;
/// The widest a card is, in cells, so its lines are easy to read; and the
/// narrowest worth setting.
const WIDEST: u16 = 76;
const NARROWEST: u16 = 30;
/// The words' size to a row's height, and smaller, in turn, where a card
/// that size is too tall for the window.
const SIZES: [f32; 3] = [0.62, 0.53, 0.45];

/// Where cosmic-text looks for the letters a face hasn't: the CJK serif
/// (its Japanese forms, as the page of type's), then its Korean.
struct Faces;

impl Fallback for Faces {
    fn common_fallback(&self) -> &[&'static str] {
        &[CJK, "Noto Serif CJK SC", SANS]
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, script: Script, _locale: &str) -> &[&'static str] {
        match script {
            Script::Hangul => &["Noto Serif CJK KR"],
            _ => &[],
        }
    }
}

/// A card, typeset: its PNG and the cells it's drawn over.
#[derive(Debug)]
pub struct Card {
    pub png: Vec<u8>,
    pub cols: u16,
    pub rows: u16,
}

/// What a card was made of, to find it again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Made {
    bulletin: String,
    kicker: String,
    cols: u16,
    cell: (u32, u32),
    most: u16,
}

/// The board's faces, and the cards it made last: most callers see the
/// same bulletin at the same size.
pub struct Typesetter {
    fonts: Mutex<(FontSystem, SwashCache)>,
    made: Mutex<Vec<(Made, Option<Arc<Card>>)>>,
}

/// The cards kept.
const KEPT: usize = 8;

impl Typesetter {
    /// The faces in `folder`: `None` where Noto Serif isn't among them.
    pub fn load(folder: &Path) -> Option<Typesetter> {
        let mut db = fontdb::Database::new();
        db.load_fonts_dir(folder);
        if !db.faces().any(|face| face.families.iter().any(|(name, _)| name == SERIF)) {
            return None;
        }
        let system = FontSystem::new_with_locale_and_db_and_fallback("en-US".into(), db, Faces);
        Some(Typesetter { fonts: Mutex::new((system, SwashCache::new())), made: Mutex::default() })
    }

    /// `bulletin` set on a card under `kicker`, for a window `cols` cells
    /// wide of `cell` pixels each, at most `most` rows tall: `None` where the
    /// window's too narrow, or the card too tall even set smaller.
    pub fn card(&self, bulletin: &str, kicker: &str, cols: u16, cell: (u32, u32), most: u16) -> Option<Arc<Card>> {
        let key = Made { bulletin: bulletin.to_string(), kicker: kicker.to_string(), cols, cell, most };
        if let Some((_, card)) = self.made.lock().unwrap().iter().find(|(k, _)| *k == key) {
            return card.clone();
        }
        let card = self.set(bulletin, kicker, cols, cell, most).map(Arc::new);
        let mut made = self.made.lock().unwrap();
        made.push((key, card.clone()));
        if made.len() > KEPT {
            made.remove(0);
        }
        card
    }

    fn set(&self, bulletin: &str, kicker: &str, cols: u16, cell: (u32, u32), most: u16) -> Option<Card> {
        let card_cols = cols.saturating_sub(4).min(WIDEST);
        let blocks = blocks(bulletin);
        if card_cols < NARROWEST || most < 4 || cell.0 == 0 || cell.1 == 0 || blocks.iter().all(|(k, _)| *k == Kind::Gap) {
            return None;
        }
        let (cw, ch) = (cell.0 * FINE, cell.1 * FINE);
        let width = card_cols as u32 * cw;
        let mut fonts = self.fonts.lock().unwrap();
        let (system, cache) = &mut *fonts;
        for size in SIZES {
            let page = Page::lay_out(system, &blocks, kicker, width as f32, ch as f32 * size);
            let rows = (page.height.ceil() as u32).div_ceil(ch);
            if rows <= most as u32 {
                let pixels = page.draw(system, cache, width, rows * ch);
                return Some(Card { png: png(width, rows * ch, &pixels)?, cols: card_cols, rows: rows as u16 });
            }
        }
        None
    }
}

/// Words set in one face and size, and where on the card they go.
struct Set {
    buffer: Buffer,
    at: (f32, f32),
    color: Color,
}

/// A card laid out: its words, its rule, how tall it is, and the size its
/// words are set at (the rest is measured from it).
struct Page {
    sets: Vec<Set>,
    rule: (f32, f32, f32, f32),
    height: f32,
    body: f32,
}

impl Page {
    fn lay_out(system: &mut FontSystem, blocks: &[(Kind, String)], kicker: &str, width: f32, body: f32) -> Page {
        let left = 2.2 * body;
        let measure = width - 2.0 * left;
        let serif = Attrs::new().family(Family::Name(SERIF));
        let bold = serif.clone().weight(Weight::BOLD);
        let mut sets = Vec::new();
        let mut y = 2.0 * body;
        // The running title, tracked out, over a red rule.
        let tracked = Attrs::new().family(Family::Name(SANS)).weight(Weight::BOLD).letter_spacing(0.28);
        let (buffer, h) = set(system, kicker, &tracked, 0.72 * body, 1.3, measure);
        sets.push(Set { buffer, at: (left, y), color: RED });
        y += h + 0.35 * body;
        let rule = (left, y, measure, (0.075 * body).max(1.5));
        y += rule.3 + 1.1 * body;
        let mut first = true;
        for (kind, words) in blocks {
            let (attrs, size, leading, before, after, indent) = match kind {
                Kind::Gap => {
                    y += 0.6 * body;
                    continue;
                }
                Kind::Headline => (&bold, 1.9 * body, 1.18, 0.6 * body, 0.35 * body, 0.0),
                Kind::Subhead => (&bold, 1.3 * body, 1.25, 0.5 * body, 0.2 * body, 0.0),
                Kind::Point => (&serif, body, 1.42, 0.0, 0.2 * body, 1.3 * body),
                Kind::Body => (&serif, body, 1.42, 0.0, 0.35 * body, 0.0),
            };
            if !first {
                y += before;
            }
            first = false;
            if *kind == Kind::Point {
                let (bullet, _) = set(system, "•", &serif, size, leading, indent);
                sets.push(Set { buffer: bullet, at: (left + 0.2 * body, y), color: RED });
            }
            let (buffer, h) = set(system, words, attrs, size, leading, measure - indent);
            sets.push(Set { buffer, at: (left + indent, y), color: INK });
            y += h + after;
        }
        Page { sets, rule, height: y + 1.6 * body, body }
    }

    /// The card's pixels, `width` × `height`, RGBA: the paper with its edge,
    /// round-cornered, clear round it; the rule; the words.
    fn draw(mut self, system: &mut FontSystem, cache: &mut SwashCache, width: u32, height: u32) -> Vec<u8> {
        let (w, h) = (width as usize, height as usize);
        let mut px = vec![0u8; w * h * 4];
        let inset = 0.3 * self.body;
        let (x0, y0, x1, y1) = (inset, inset, w as f32 - inset, h as f32 - inset);
        let radius = 0.9 * self.body;
        let stroke = (0.09 * self.body).max(1.5);
        for y in 0..h {
            for x in 0..w {
                let d = rounded((x as f32 + 0.5, y as f32 + 0.5), (x0, y0, x1, y1), radius);
                let at = (y * w + x) * 4;
                over(&mut px[at..at + 4], PAPER, (0.5 - d).clamp(0.0, 1.0));
                over(&mut px[at..at + 4], EDGE, (0.5 - ((d + stroke / 2.0).abs() - stroke / 2.0)).clamp(0.0, 1.0));
            }
        }
        let (rx, ry, rw, rh) = self.rule;
        fill(&mut px, w, h, (rx, ry, rw, rh), [0xb5, 0x32, 0x2a]);
        for s in &mut self.sets {
            let (ox, oy) = (s.at.0.round() as i32, s.at.1.round() as i32);
            s.buffer.draw(system, cache, s.color, |x, y, bw, bh, color| {
                let a = color.a() as f32 / 255.0;
                for yy in y..y + bh as i32 {
                    for xx in x..x + bw as i32 {
                        let (px_x, px_y) = (xx + ox, yy + oy);
                        if px_x >= 0 && px_y >= 0 && (px_x as usize) < w && (px_y as usize) < h {
                            let at = (px_y as usize * w + px_x as usize) * 4;
                            over(&mut px[at..at + 4], [color.r(), color.g(), color.b()], a);
                        }
                    }
                }
            });
        }
        px
    }
}

/// `words` with a typesetter's quotes for a typewriter's: an apostrophe
/// and a closing quote curl back, an opening one forward.
fn curly(words: &str) -> String {
    let mut out = String::with_capacity(words.len());
    let mut before: Option<char> = None;
    for c in words.chars() {
        let opens = before.is_none_or(|b| b.is_whitespace() || "([{—–".contains(b));
        out.push(match c {
            '\'' if opens => '‘',
            '\'' => '’',
            '"' if opens => '“',
            '"' => '”',
            c => c,
        });
        before = Some(c);
    }
    out
}

/// `words` set at `size` pixels, lines `leading` times that apart, broken
/// to `width`: the words, and how tall they are.
fn set(system: &mut FontSystem, words: &str, attrs: &Attrs, size: f32, leading: f32, width: f32) -> (Buffer, f32) {
    let mut buffer = Buffer::new(system, Metrics::new(size, size * leading));
    buffer.set_size(Some(width.max(1.0)), None);
    buffer.set_text(&curly(words), attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    let height = buffer.layout_runs().map(|run| run.line_top + run.line_height).fold(0.0, f32::max);
    (buffer, height)
}

/// How far a point is outside a round-cornered box (less than nothing
/// inside it).
fn rounded((x, y): (f32, f32), (x0, y0, x1, y1): (f32, f32, f32, f32), r: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (qx, qy) = ((x - cx).abs() - ((x1 - x0) / 2.0 - r), (y - cy).abs() - ((y1 - y0) / 2.0 - r));
    (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r
}

/// A color laid over a pixel at `a` of its strength.
fn over(px: &mut [u8], rgb: [u8; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let below = px[3] as f32 / 255.0;
    let out = a + below * (1.0 - a);
    for i in 0..3 {
        px[i] = ((rgb[i] as f32 * a + px[i] as f32 * below * (1.0 - a)) / out).round() as u8;
    }
    px[3] = (out * 255.0).round() as u8;
}

/// A box of `rgb`, its edges as much of a pixel as they cover.
fn fill(px: &mut [u8], w: usize, h: usize, (x, y, bw, bh): (f32, f32, f32, f32), rgb: [u8; 3]) {
    let cover = |p: usize, from: f32, to: f32| (to.min(p as f32 + 1.0) - from.max(p as f32)).clamp(0.0, 1.0);
    for py in (y.floor() as usize)..((y + bh).ceil() as usize).min(h) {
        for px_x in (x.floor() as usize)..((x + bw).ceil() as usize).min(w) {
            let a = cover(px_x, x, x + bw) * cover(py, y, y + bh);
            let at = (py * w + px_x) * 4;
            over(&mut px[at..at + 4], rgb, a);
        }
    }
}

fn png(width: u32, height: u32, pixels: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(pixels).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bulletins_lines_are_read_by_their_marks() {
        let b = blocks("# Welcome\n\n## News\n- one\n* two\nplain words  \n#not a headline");
        let kinds: Vec<Kind> = b.iter().map(|(k, _)| *k).collect();
        assert_eq!(kinds, [Kind::Headline, Kind::Gap, Kind::Subhead, Kind::Point, Kind::Point, Kind::Body, Kind::Body]);
        assert_eq!((b[0].1.as_str(), b[5].1.as_str(), b[6].1.as_str()), ("Welcome", "plain words", "#not a headline"));
        assert_eq!(blocks("\x1b[2Jhi")[0].1, "[2Jhi", "no escapes");
    }

    #[test]
    fn as_text_a_headline_is_colored_and_a_point_hangs_from_its_bullet() {
        let lines = text("# Hello\n- a point long enough to wrap\nbody", 14);
        assert_eq!(lines[0], format!("{CYAN}Hello{PLAIN}"));
        assert_eq!(&lines[1..4], ["• a point long", "  enough to", "  wrap"]);
        assert_eq!(lines[4], "body");
    }

    #[test]
    fn quotes_and_apostrophes_are_set_curled() {
        assert_eq!(curly(r#"what's "this", 'it' (said "so")"#), "what’s “this”, ‘it’ (said “so”)");
        assert_eq!(curly("'90s rock'n'roll"), "‘90s rock’n’roll", "an elision at a word's start can't be told from a quote");
    }

    #[test]
    fn a_round_cornered_box_is_measured_from_its_edges_and_its_corners() {
        let b = (0.0, 0.0, 100.0, 50.0);
        assert!(rounded((50.0, 25.0), b, 10.0) < -20.0, "deep inside");
        assert!((rounded((50.0, -2.0), b, 10.0) - 2.0).abs() < 1e-4, "two over the top edge");
        assert!(rounded((1.0, 1.0), b, 10.0) > 0.0, "the corner's cut away");
    }

    #[test]
    fn laid_over_clear_a_color_is_itself_and_over_another_it_mixes() {
        let mut px = [0, 0, 0, 0];
        over(&mut px, [200, 100, 50], 1.0);
        assert_eq!(px, [200, 100, 50, 255]);
        over(&mut px, [0, 0, 0], 0.5);
        assert_eq!(px, [100, 50, 25, 255]);
        let mut px = [0, 0, 0, 0];
        over(&mut px, [10, 20, 30], 0.25);
        assert_eq!(px, [10, 20, 30, 64], "clear under it: its color, a quarter there");
    }

    /// The faces the board's image carries, where a test's machine has them
    /// (`ANIE_BBS_TEST_FONTS`); else there's nothing to set with, and the
    /// test says so and passes.
    fn faces() -> Option<Typesetter> {
        let folder = std::env::var_os("ANIE_BBS_TEST_FONTS")?;
        Some(Typesetter::load(Path::new(&folder)).expect("ANIE_BBS_TEST_FONTS has no Noto Serif"))
    }

    #[test]
    fn a_card_is_a_whole_number_of_rows_tall_at_twice_the_cells_pixels() {
        let Some(t) = faces() else {
            eprintln!("no ANIE_BBS_TEST_FONTS: nothing set");
            return;
        };
        let bulletin = "# Welcome to the board\nSay hello in General, ask in Support, and file what's broken in the bug tracker.\n\n## This week\n- Dungeon is open in the game room.\n- 東京と香港から、端末に活字を。";
        let card = t.card(bulletin, "THE ÁNIË SOLUTIONS BBS · BULLETIN", 100, (10, 21), 40).expect("a card");
        let (w, h) = (u32::from_be_bytes(card.png[16..20].try_into().unwrap()), u32::from_be_bytes(card.png[20..24].try_into().unwrap()));
        assert_eq!((card.cols, w), (WIDEST, WIDEST as u32 * 20), "as wide as it's let be, at twice a cell's pixels");
        assert_eq!(h, card.rows as u32 * 42, "{h} pixels over {} rows", card.rows);
        assert!((6..=40).contains(&card.rows), "{}", card.rows);
        assert!(Arc::ptr_eq(&card, &t.card(bulletin, "THE ÁNIË SOLUTIONS BBS · BULLETIN", 100, (10, 21), 40).unwrap()), "made once");
        if let Some(out) = std::env::var_os("ANIE_BBS_TEST_CARD") {
            std::fs::write(out, &card.png).unwrap();
        }
        // Too few rows even set small, or too narrow a window: none.
        assert!(t.card(bulletin, "K", 100, (10, 21), 5).is_none());
        assert!(t.card(bulletin, "K", 30, (10, 21), 40).is_none());
        let small = t.card(bulletin, "K", 100, (10, 21), card.rows - 2).expect("set smaller to fit");
        assert!(small.rows <= card.rows - 2);
    }
}
