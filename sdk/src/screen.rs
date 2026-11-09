//! Screens: what a board shows, written as text files a sysop edits. A
//! screen is lines of text, with marks in braces:
//!
//! - `{user}`, `{board}`, or any name the board gives a value: the value.
//! - `{red}`, `{green}`, `{yellow}`, `{blue}`, `{magenta}`, `{cyan}`,
//!   `{white}`, `{black}`, each also `{bright-…}`, and `{bold}`, `{dim}`,
//!   `{/}` to go back to plain: the sixteen colors, so a caller's theme
//!   recolors them, as Flow-Utils' do.
//! - `{sound chime}`: one of Flow's named sounds, there.
//! - `{fx ripple}` (`burst`, `stir`, `bolt`): an effect on the glass at
//!   that line.
//!
//! A line that's only `![words](file)` is a picture (a PNG) or a turning
//! model (`.obj`, `.glb`), its file from the board's folder. A caller in
//! Flow sees them, and hears the sounds and sees the effects; any other
//! caller sees the words in brackets, and the colors. `{{` is a brace.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use flow_sdk::fx::{Effect, Kind, Sound};
use flow_sdk::model::{self, Format, Look};
use flow_sdk::picture::{self, Image};

use crate::call::{Call, Shown};

/// A screen, read and ready to show to any number of calls.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    pub(crate) lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Line {
    Text(Vec<Piece>),
    Picture { words: String, png: Arc<Vec<u8>>, size: (u32, u32) },
    Model { words: String, bytes: Arc<Vec<u8>>, format: Format },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Piece {
    Text(String),
    /// An SGR's parameters: `31`, `1`, `0`.
    Style(&'static str),
    /// A value the board gives when it shows the screen.
    Name(String),
    Sound(Sound),
    Effect(Kind),
}

/// The colors and styles a screen may name, as SGR parameters.
const STYLES: &[(&str, &str)] = &[
    ("/", "0"),
    ("bold", "1"),
    ("dim", "2"),
    ("black", "30"),
    ("red", "31"),
    ("green", "32"),
    ("yellow", "33"),
    ("blue", "34"),
    ("magenta", "35"),
    ("cyan", "36"),
    ("white", "37"),
    ("bright-black", "90"),
    ("bright-red", "91"),
    ("bright-green", "92"),
    ("bright-yellow", "93"),
    ("bright-blue", "94"),
    ("bright-magenta", "95"),
    ("bright-cyan", "96"),
    ("bright-white", "97"),
];

const EFFECTS: &[(&str, Kind)] = &[("ripple", Kind::Ripple), ("burst", Kind::Burst), ("stir", Kind::Stir), ("bolt", Kind::Bolt)];

impl Screen {
    /// Reads the screen in `file`, its pictures and models from `folder`.
    pub fn read(file: &Path, folder: &Path) -> io::Result<Screen> {
        let text = fs::read_to_string(file).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", file.display())))?;
        Screen::parse(&text, folder).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", file.display())))
    }

    /// A screen from its text, its pictures and models from `folder`.
    pub fn parse(text: &str, folder: &Path) -> Result<Screen, String> {
        let mut lines = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let at = |e: String| format!("line {}: {e}", n + 1);
            lines.push(match picture(line) {
                Some((words, file)) => media(words, file, folder).map_err(at)?,
                None => Line::Text(pieces(line).map_err(at)?),
            });
        }
        Ok(Screen { lines })
    }

    /// The names the screen asks the board for, `{user}` and the like.
    pub fn names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.lines.iter().flat_map(|l| match l {
            Line::Text(pieces) => pieces.iter().filter_map(|p| if let Piece::Name(n) = p { Some(n.as_str()) } else { None }).collect(),
            _ => Vec::new(),
        }).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// `![words](file)` alone on a line.
fn picture(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("![")?.strip_suffix(')')?;
    let (words, file) = rest.split_once("](")?;
    Some((words, file))
}

fn media(words: &str, file: &str, folder: &Path) -> Result<Line, String> {
    let path = folder.join(file);
    let bytes = fs::read(&path).map_err(|e| format!("{file}: {e}"))?;
    let words = words.to_string();
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".png") {
        let size = png_size(&bytes).ok_or(format!("{file} isn't a PNG"))?;
        Ok(Line::Picture { words, png: Arc::new(bytes), size })
    } else if lower.ends_with(".glb") {
        Ok(Line::Model { words, bytes: Arc::new(bytes), format: Format::Glb })
    } else if lower.ends_with(".obj") {
        Ok(Line::Model { words, bytes: Arc::new(bytes), format: Format::Obj })
    } else {
        Err(format!("{file}: a picture is a .png, a model a .obj or .glb"))
    }
}

/// A PNG's width and height, from its header.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16) != Some(b"IHDR") {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some((be(16), be(20))).filter(|&(w, h)| w > 0 && h > 0)
}

/// A line of text's pieces.
fn pieces(line: &str) -> Result<Vec<Piece>, String> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = line;
    while let Some(i) = rest.find('{') {
        text.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        if let Some(after) = rest.strip_prefix('{') {
            text.push('{');
            rest = after;
            continue;
        }
        let Some(end) = rest.find('}') else { return Err("a { with no } after it (write {{ for a brace)".into()) };
        let mark = rest[..end].trim();
        rest = &rest[end + 1..];
        if !text.is_empty() {
            out.push(Piece::Text(std::mem::take(&mut text)));
        }
        out.push(mark_piece(mark)?);
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    Ok(out)
}

fn mark_piece(mark: &str) -> Result<Piece, String> {
    if let Some(&(_, sgr)) = STYLES.iter().find(|(name, _)| *name == mark) {
        return Ok(Piece::Style(sgr));
    }
    if let Some(name) = mark.strip_prefix("sound ") {
        return Sound::named(name.trim()).map(Piece::Sound).ok_or(format!("{{sound {name}}}: Flow has no sound of that name"));
    }
    if let Some(name) = mark.strip_prefix("fx ") {
        return EFFECTS.iter().find(|(n, _)| *n == name.trim()).map(|&(_, k)| Piece::Effect(k)).ok_or(format!("{{fx {name}}}: an effect is ripple, burst, stir or bolt"));
    }
    if !mark.is_empty() && mark.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Ok(Piece::Name(mark.to_string()));
    }
    Err(format!("{{{mark}}} isn't a color, a sound, an effect or a name"))
}

/// What a line of text writes: its words and colors, and for a caller in
/// Flow its sounds and effects, placed at `row` in a window `cols` wide.
pub(crate) fn text(pieces: &[Piece], values: &[(&str, &str)], flow: bool, row: u16, cols: u16) -> String {
    let mut out = String::new();
    let mut col = 0u16;
    for piece in pieces {
        match piece {
            Piece::Text(t) => {
                out.push_str(t);
                col = col.saturating_add(t.chars().count() as u16);
            }
            Piece::Style(sgr) => out.push_str(&format!("\x1b[{sgr}m")),
            Piece::Name(n) => {
                let value = values.iter().find(|(k, _)| k == n).map_or_else(|| format!("{{{n}}}"), |(_, v)| v.to_string());
                col = col.saturating_add(value.chars().count() as u16);
                out.push_str(&value);
            }
            Piece::Sound(s) if flow => out.push_str(&s.escape(Some((col, row)), None)),
            Piece::Effect(k) if flow => {
                let at = (col.max(cols / 2).min(cols.saturating_sub(1)), row);
                let effect = match k {
                    Kind::Bolt => Effect::bolt((at.0, 0), at),
                    Kind::Stir => Effect::stir(at, (1.0, 0.0)),
                    Kind::Burst => Effect::burst(at),
                    Kind::Ripple => Effect::ripple(at),
                };
                out.push_str(&effect.escape());
            }
            Piece::Sound(_) | Piece::Effect(_) => {}
        }
    }
    out.push_str("\x1b[0m");
    out
}

/// A cell's pixels where the caller's window didn't say: a common size.
const CELL: (u32, u32) = (9, 18);

/// How many rows a model takes.
const MODEL_ROWS: u16 = 10;

impl Call {
    /// Clears the caller's screen and shows `screen` from its top, `values`
    /// filling its names (`{user}` is the caller's login unless given).
    /// Pictures and models the last screen showed are forgotten first.
    /// Gives the row it ends on, from 0.
    pub fn show(&mut self, screen: &Screen, values: &[(&str, &str)]) -> io::Result<u16> {
        self.forget()?;
        let user = self.user().to_string();
        let mut named: Vec<(&str, &str)> = values.to_vec();
        if !named.iter().any(|(k, _)| *k == "user") {
            named.push(("user", &user));
        }
        let (flow, size) = (self.is_flow(), self.size());
        let cols = if size.cols == 0 { 80 } else { size.cols };
        let rows = if size.rows == 0 { 24 } else { size.rows };
        let cell = size.cell().filter(|&(w, h)| w > 0 && h > 0).unwrap_or(CELL);
        let mut out = String::from("\x1b[2J\x1b[H");
        let mut row = 0u16;
        for line in &screen.lines {
            match line {
                Line::Text(pieces) => {
                    out.push_str(&text(pieces, &named, flow, row, cols));
                    out.push_str("\r\n");
                    row += 1;
                }
                Line::Picture { png, size: (w, h), .. } if flow => {
                    let (pc, pr) = fit((*w, *h), cell, cols, rows.saturating_sub(row + 1).max(1));
                    let id = self.next_id();
                    out.push_str(&picture::upload(id, &Image::png(png.to_vec()), 1, pc, pr));
                    out.push_str(&picture::placeholders(id, 1, (0, row), pc, pr));
                    row += pr;
                    out.push_str(&format!("\x1b[{};1H", row + 1));
                    self.shown.push(Shown::Picture(id));
                }
                Line::Model { words, bytes, format } if flow => {
                    let mr = MODEL_ROWS.min(rows.saturating_sub(row + 1)).max(1);
                    let mc = (mr as u32 * cell.1 / cell.0).clamp(1, cols as u32) as u16;
                    let id = self.next_id();
                    // Room for it first, then the model over those cells.
                    out.push_str(&"\r\n".repeat(mr as usize));
                    out.push_str(&model::register_chunks(id, *format, bytes, Some(words), 3072));
                    out.push_str(&model::Place::over(id, (row, 0), (mc, mr)).look(Look { animate: Some(true), ..Look::default() }).escape());
                    row += mr;
                    self.shown.push(Shown::Model(id));
                }
                Line::Picture { words, .. } | Line::Model { words, .. } => {
                    out.push_str(&format!("\x1b[2m[{words}]\x1b[0m\r\n"));
                    row += 1;
                }
            }
        }
        self.send(&out)?;
        Ok(row)
    }

    /// Forgets the pictures and models the call put on the caller's
    /// screen, so a Flow that's had many screens holds only the last's.
    pub(crate) fn forget(&mut self) -> io::Result<()> {
        let gone: String = self.shown.drain(..).map(|s| match s {
            Shown::Picture(id) => picture::delete(id),
            Shown::Model(id) => model::delete(id),
        }).collect();
        if gone.is_empty() {
            Ok(())
        } else {
            self.send(&gone)
        }
    }
}

/// A picture of `w` × `h` pixels in cells of `cell`: its own size where it
/// fits in `cols` × `rows`, else as big as fits.
fn fit((w, h): (u32, u32), cell: (u32, u32), cols: u16, rows: u16) -> (u16, u16) {
    let up = |a: u32, b: u32| a.div_ceil(b.max(1));
    let (mut c, mut r) = (up(w, cell.0).max(1), up(h, cell.1).max(1));
    if c > cols as u32 {
        r = up(r * cols as u32, c).max(1);
        c = cols as u32;
    }
    if r > rows as u32 {
        c = up(c * rows as u32, r).max(1);
        r = rows as u32;
    }
    (c as u16, r as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_its_own_size_in_cells_where_it_fits_else_as_big_as_fits() {
        assert_eq!(fit((640, 160), (10, 20), 80, 24), (64, 8));
        assert_eq!(fit((1600, 400), (10, 20), 80, 24), (80, 10), "too wide: as wide as the window");
        assert_eq!(fit((200, 1000), (10, 20), 80, 24), (10, 24), "too tall: as tall as there's room");
        assert_eq!(fit((1, 1), (10, 20), 80, 24), (1, 1));
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    fn folder() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("flow-bbs-screen-{}", std::process::id()));
        fs::create_dir_all(dir.join("pictures")).unwrap();
        fs::write(dir.join("pictures/banner.png"), png(640, 160)).unwrap();
        fs::write(dir.join("pictures/cube.obj"), "v 0 0 0\n").unwrap();
        dir
    }

    #[test]
    fn marks_become_colors_names_sounds_and_effects() {
        let s = Screen::parse("{bold}Hi{/}, {user}! {sound chime}{fx ripple} {{x}", Path::new(".")).unwrap();
        assert_eq!(
            s.lines,
            [Line::Text(vec![
                Piece::Style("1"),
                Piece::Text("Hi".into()),
                Piece::Style("0"),
                Piece::Text(", ".into()),
                Piece::Name("user".into()),
                Piece::Text("! ".into()),
                Piece::Sound(Sound::Chime),
                Piece::Effect(Kind::Ripple),
                Piece::Text(" {x}".into()),
            ])]
        );
        assert_eq!(s.names(), ["user"]);
    }

    #[test]
    fn a_line_that_is_a_picture_or_a_model_reads_its_file() {
        let dir = folder();
        let s = Screen::parse("![The board](pictures/banner.png)\n  ![A cube](pictures/cube.obj)  ", &dir).unwrap();
        assert!(matches!(&s.lines[0], Line::Picture { words, size: (640, 160), .. } if words == "The board"));
        assert!(matches!(&s.lines[1], Line::Model { format: Format::Obj, .. }));
    }

    #[test]
    fn a_mistake_is_said_with_its_line() {
        let dir = folder();
        for (text, says) in [
            ("ok\n{sound kazoo}", "line 2: {sound kazoo}: Flow has no sound of that name"),
            ("{fx quake}", "line 1: {fx quake}: an effect is ripple, burst, stir or bolt"),
            ("{not a mark}", "line 1: {not a mark} isn't a color, a sound, an effect or a name"),
            ("a { b", "line 1: a { with no } after it (write {{ for a brace)"),
            ("![x](pictures/none.png)", "line 1: pictures/none.png: "),
            ("![x](pictures/cube.obj.txt)", "line 1: pictures/cube.obj.txt: "),
        ] {
            let e = Screen::parse(text, &dir).unwrap_err();
            assert!(e.starts_with(says), "{text:?}: {e}");
        }
    }

    #[test]
    fn a_caller_in_flow_gets_the_sounds_and_effects_and_another_the_words_and_colors() {
        let s = Screen::parse("{green}Hello, {user}{sound chime}{/}", Path::new(".")).unwrap();
        let Line::Text(pieces) = &s.lines[0] else { panic!() };
        let plain = text(pieces, &[("user", "guest")], false, 2, 80);
        assert_eq!(plain, "\x1b[32mHello, guest\x1b[0m\x1b[0m");
        let flow = text(pieces, &[("user", "guest")], true, 2, 80);
        assert!(flow.contains("\x1b]7731;fx=sound;sound=chime"), "{flow:?}");
        assert_eq!(text(pieces, &[], false, 0, 80), "\x1b[32mHello, {user}\x1b[0m\x1b[0m", "a name not given stays as written");
    }
}
