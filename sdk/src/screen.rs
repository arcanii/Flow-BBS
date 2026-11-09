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
//!
//! A picture is its own size in cells, or `{rows 10}` after it makes it
//! that many rows tall, as wide as that makes it, whatever the size of
//! the caller's cells (a model is 10 rows unless it says). Either is made
//! smaller where it would leave no room for the lines under it, or for
//! what the board writes after the screen (a menu's items).
//!
//! A line that starts `{in flow}` is shown only to a caller in Flow, and
//! one that starts `{not in flow}` only to any other: a picture for one,
//! say, and letters drawn for the other.

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
    pub(crate) lines: Vec<(Who, Line)>,
}

/// Who sees a line: every caller, or only a caller in Flow, or only one
/// not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Who {
    All,
    Flow,
    Others,
}

impl Who {
    fn sees(self, flow: bool) -> bool {
        match self {
            Who::All => true,
            Who::Flow => flow,
            Who::Others => !flow,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Line {
    Text(Vec<Piece>),
    /// `rows`: how tall it's asked to be, where it says.
    Picture { words: String, png: Arc<Vec<u8>>, size: (u32, u32), rows: Option<u16> },
    Model { words: String, bytes: Arc<Vec<u8>>, format: Format, rows: Option<u16> },
}

impl Line {
    /// The rows the line takes for a caller in Flow, a picture at least.
    fn rows(&self) -> u16 {
        match self {
            Line::Text(_) => 1,
            Line::Picture { rows, .. } => rows.unwrap_or(1),
            Line::Model { rows, .. } => rows.unwrap_or(MODEL_ROWS),
        }
    }
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
            let (who, line) = who(line);
            lines.push((who, match picture(line) {
                Some((words, file, rows)) => media(words, file, rows, folder).map_err(at)?,
                None => Line::Text(pieces(line).map_err(at)?),
            }));
        }
        Ok(Screen { lines })
    }

    /// The names the screen asks the board for, `{user}` and the like.
    pub fn names(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.lines.iter().flat_map(|(_, l)| match l {
            Line::Text(pieces) => pieces.iter().filter_map(|p| if let Piece::Name(n) = p { Some(n.as_str()) } else { None }).collect(),
            _ => Vec::new(),
        }).collect();
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Who a line is for, from `{in flow}` or `{not in flow}` at its start,
/// and the rest of it.
fn who(line: &str) -> (Who, &str) {
    if let Some(rest) = line.strip_prefix("{in flow}") {
        (Who::Flow, rest)
    } else if let Some(rest) = line.strip_prefix("{not in flow}") {
        (Who::Others, rest)
    } else {
        (Who::All, line)
    }
}

/// `![words](file)` alone on a line, and what `{rows …}` after it says.
fn picture(line: &str) -> Option<(&str, &str, Option<&str>)> {
    let mut line = line.trim();
    let mut rows = None;
    if let Some(start) = line.rfind("{rows ").filter(|_| line.ends_with('}')) {
        rows = Some(line[start + "{rows ".len()..line.len() - 1].trim());
        line = line[..start].trim_end();
    }
    let rest = line.strip_prefix("![")?.strip_suffix(')')?;
    let (words, file) = rest.split_once("](")?;
    Some((words, file, rows))
}

fn media(words: &str, file: &str, rows: Option<&str>, folder: &Path) -> Result<Line, String> {
    let rows = match rows {
        Some(r) => Some(r.parse::<u16>().ok().filter(|&n| n > 0).ok_or(format!("{{rows {r}}}: rows are a number, 1 or more"))?),
        None => None,
    };
    let path = folder.join(file);
    let bytes = fs::read(&path).map_err(|e| format!("{file}: {e}"))?;
    let words = words.to_string();
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".png") {
        let size = png_size(&bytes).ok_or(format!("{file} isn't a PNG"))?;
        Ok(Line::Picture { words, png: Arc::new(bytes), size, rows })
    } else if lower.ends_with(".glb") {
        Ok(Line::Model { words, bytes: Arc::new(bytes), format: Format::Glb, rows })
    } else if lower.ends_with(".obj") {
        Ok(Line::Model { words, bytes: Arc::new(bytes), format: Format::Obj, rows })
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
    if mark == "in flow" || mark == "not in flow" {
        return Err(format!("{{{mark}}} goes at the start of a line"));
    }
    if mark.starts_with("rows ") {
        return Err(format!("{{{mark}}} goes after a picture or a model, on its line"));
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
        self.show_leaving(screen, values, 1)
    }

    /// `show`, leaving `below` rows under the screen for what the board
    /// writes after it (a menu leaves its items room): a picture or a model
    /// is made smaller where the window has too few rows for it and them.
    pub fn show_leaving(&mut self, screen: &Screen, values: &[(&str, &str)], below: u16) -> io::Result<u16> {
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
        let lines: Vec<&Line> = screen.lines.iter().filter(|(who, _)| who.sees(flow)).map(|(_, line)| line).collect();
        for (i, line) in lines.iter().enumerate() {
            // The rows a picture or a model may take: what the lines under
            // it, and what comes after the screen, leave.
            let under = lines[i + 1..].iter().fold(below, |n, l| n.saturating_add(l.rows()));
            let room = rows.saturating_sub(row.saturating_add(under)).max(1);
            match line {
                Line::Text(pieces) => {
                    out.push_str(&text(pieces, &named, flow, row, cols));
                    out.push_str("\r\n");
                    row += 1;
                }
                Line::Picture { png, size: (w, h), rows: tall, .. } if flow => {
                    let (pc, pr) = fit((*w, *h), cell, *tall, cols, room);
                    let id = self.next_id();
                    out.push_str(&picture::upload(id, &Image::png(png.to_vec()), 1, pc, pr));
                    out.push_str(&picture::placeholders(id, 1, (0, row), pc, pr));
                    row += pr;
                    out.push_str(&format!("\x1b[{};1H", row + 1));
                    self.shown.push(Shown::Picture(id));
                }
                Line::Model { words, bytes, format, rows: tall } if flow => {
                    let mr = tall.unwrap_or(MODEL_ROWS).min(room);
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

    /// Clears the caller's screen to draw a board's own on it, the last
    /// screen's pictures and models forgotten as `show` forgets them.
    pub fn clear(&mut self) -> io::Result<()> {
        self.forget()?;
        self.send("[2J[H")
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

/// A picture of `w` × `h` pixels in cells of `cell`: `tall` rows where
/// that's asked, else its own size, where it fits in `cols` × `rows`; else
/// as big as fits.
fn fit((w, h): (u32, u32), cell: (u32, u32), tall: Option<u16>, cols: u16, rows: u16) -> (u16, u16) {
    let up = |a: u64, b: u64| a.div_ceil(b.max(1));
    let (w, h, cw, ch) = (w as u64, h as u64, cell.0 as u64, cell.1 as u64);
    let (mut c, mut r) = match tall {
        // Its width in pixels at that height, in cells. (A caller's window
        // says the size of its cells, so nothing here may overflow.)
        Some(t) => (up(w.saturating_mul(t as u64).saturating_mul(ch), h.saturating_mul(cw)).max(1), t as u64),
        None => (up(w, cw).max(1), up(h, ch).max(1)),
    };
    if c > cols as u64 {
        r = up(r.saturating_mul(cols as u64), c).max(1);
        c = cols as u64;
    }
    if r > rows as u64 {
        c = up(c.saturating_mul(rows as u64), r).max(1);
        r = rows as u64;
    }
    (c as u16, r as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_its_own_size_in_cells_where_it_fits_else_as_big_as_fits() {
        assert_eq!(fit((640, 160), (10, 20), None, 80, 24), (64, 8));
        assert_eq!(fit((1600, 400), (10, 20), None, 80, 24), (80, 10), "too wide: as wide as the window");
        assert_eq!(fit((200, 1000), (10, 20), None, 80, 24), (10, 24), "too tall: as tall as there's room");
        assert_eq!(fit((1, 1), (10, 20), None, 80, 24), (1, 1));
    }

    #[test]
    fn a_picture_given_its_rows_is_that_tall_whatever_the_cells_and_still_fits() {
        assert_eq!(fit((1038, 400), (16, 38), Some(10), 120, 40), (62, 10), "a HiDPI screen's cells");
        assert_eq!(fit((1038, 400), (9, 18), Some(10), 120, 40), (52, 10), "the same rows, narrower cells");
        assert_eq!(fit((1038, 400), (9, 18), Some(10), 40, 40), (40, 8), "too wide: as wide as the window");
        assert_eq!(fit((1038, 400), (9, 18), Some(10), 120, 6), (32, 6), "too tall: as tall as there's room");
        assert_eq!(fit((u32::MAX, 1), (u32::MAX, u32::MAX), Some(u16::MAX), 80, 24), (80, 1), "no size overflows");
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    /// A folder of the test's own, so tests running at once never write
    /// a picture another is reading.
    fn folder(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("flow-bbs-screen-{}-{test}", std::process::id()));
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
            [(Who::All, Line::Text(vec![
                Piece::Style("1"),
                Piece::Text("Hi".into()),
                Piece::Style("0"),
                Piece::Text(", ".into()),
                Piece::Name("user".into()),
                Piece::Text("! ".into()),
                Piece::Sound(Sound::Chime),
                Piece::Effect(Kind::Ripple),
                Piece::Text(" {x}".into()),
            ]))]
        );
        assert_eq!(s.names(), ["user"]);
    }

    #[test]
    fn a_line_that_is_a_picture_or_a_model_reads_its_file() {
        let dir = folder("media");
        let s = Screen::parse("![The board](pictures/banner.png)\n  ![A cube](pictures/cube.obj)  \n![Tall](pictures/banner.png){rows 12}\n![Short](pictures/cube.obj) {rows 4}", &dir).unwrap();
        assert!(matches!(&s.lines[0].1, Line::Picture { words, size: (640, 160), rows: None, .. } if words == "The board"));
        assert!(matches!(&s.lines[1].1, Line::Model { format: Format::Obj, rows: None, .. }));
        assert!(matches!(&s.lines[2].1, Line::Picture { rows: Some(12), .. }));
        assert!(matches!(&s.lines[3].1, Line::Model { rows: Some(4), .. }));
    }

    #[test]
    fn a_line_can_be_for_a_caller_in_flow_or_for_the_others() {
        let dir = folder("who");
        let s = Screen::parse("{in flow}![The board](pictures/banner.png){rows 6}\n{not in flow}{bold}THE BOARD{/}\nHello", &dir).unwrap();
        let who: Vec<Who> = s.lines.iter().map(|(w, _)| *w).collect();
        assert_eq!(who, [Who::Flow, Who::Others, Who::All]);
        assert!(matches!(&s.lines[0].1, Line::Picture { rows: Some(6), .. }));
        assert_eq!(s.lines[1].1, Line::Text(vec![Piece::Style("1"), Piece::Text("THE BOARD".into()), Piece::Style("0")]));
    }

    /// A call to `screen`, in Flow or not, its window 80 × 24 cells of 10
    /// × 20 pixels, and what it's sent.
    fn shown(screen: &Screen, flow: bool, below: u16) -> String {
        let (_tx, rx) = std::sync::mpsc::channel();
        let (out, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let env = if flow { vec![("TERM_PROGRAM".to_string(), crate::call::FLOW.to_string())] } else { Vec::new() };
        let mut call = Call::new("guest".into(), None, "xterm-256color".into(), crate::Size { cols: 80, rows: 24, width: 800, height: 480 }, env, rx, out);
        if flow {
            call.find_caller(std::time::Duration::from_millis(10));
        }
        call.show_leaving(screen, &[], below).unwrap();
        let mut all = Vec::new();
        while let Ok(crate::call::Out::Data(bytes)) = sent.try_recv() {
            all.extend(bytes);
        }
        String::from_utf8(all).unwrap()
    }

    #[test]
    fn each_caller_sees_their_own_lines_and_a_picture_leaves_room_under_it() {
        let dir = folder("room");
        let s = Screen::parse("{in flow}![The board](pictures/banner.png){rows 10}\n{not in flow}THE BOARD\n\nWelcome", &dir).unwrap();
        let flow = shown(&s, true, 10);
        assert!(flow.contains(",c=80,r=10,"), "ten rows, with the two lines and ten rows under it room: {flow:.200}");
        assert!(!flow.contains("THE BOARD") && flow.contains("Welcome"));
        let squeezed = shown(&s, true, 16);
        assert!(squeezed.contains(",c=48,r=6,"), "six rows where the rest leaves only those: {squeezed:.200}");
        let other = shown(&s, false, 10);
        assert!(other.contains("THE BOARD") && other.contains("Welcome") && !other.contains("The board") && !other.contains("\x1b_G"), "{other:?}");
    }

    #[test]
    fn a_mistake_is_said_with_its_line() {
        let dir = folder("mistakes");
        for (text, says) in [
            ("ok\n{sound kazoo}", "line 2: {sound kazoo}: Flow has no sound of that name"),
            ("{fx quake}", "line 1: {fx quake}: an effect is ripple, burst, stir or bolt"),
            ("{not a mark}", "line 1: {not a mark} isn't a color, a sound, an effect or a name"),
            ("a { b", "line 1: a { with no } after it (write {{ for a brace)"),
            ("![x](pictures/none.png)", "line 1: pictures/none.png: "),
            ("![x](pictures/cube.obj.txt)", "line 1: pictures/cube.obj.txt: "),
            ("![x](pictures/banner.png){rows 0}", "line 1: {rows 0}: rows are a number, 1 or more"),
            ("![x](pictures/banner.png){rows many}", "line 1: {rows many}: rows are a number, 1 or more"),
            ("Hello {rows 3}", "line 1: {rows 3} goes after a picture or a model, on its line"),
            ("Hello {in flow}", "line 1: {in flow} goes at the start of a line"),
        ] {
            let e = Screen::parse(text, &dir).unwrap_err();
            assert!(e.starts_with(says), "{text:?}: {e}");
        }
    }

    #[test]
    fn a_caller_in_flow_gets_the_sounds_and_effects_and_another_the_words_and_colors() {
        let s = Screen::parse("{green}Hello, {user}{sound chime}{/}", Path::new(".")).unwrap();
        let Line::Text(pieces) = &s.lines[0].1 else { panic!() };
        let plain = text(pieces, &[("user", "guest")], false, 2, 80);
        assert_eq!(plain, "\x1b[32mHello, guest\x1b[0m\x1b[0m");
        let flow = text(pieces, &[("user", "guest")], true, 2, 80);
        assert!(flow.contains("\x1b]7731;fx=sound;sound=chime"), "{flow:?}");
        assert_eq!(text(pieces, &[], false, 0, 80), "\x1b[32mHello, {user}\x1b[0m\x1b[0m", "a name not given stays as written");
    }
}
