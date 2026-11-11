//! A full-screen editor for what a caller writes: a message, a post, a
//! bulletin. The text fills the window between a bar across the top (what
//! it is, and where the cursor is in it) and the keys along the bottom, and
//! wraps at its words to the window as it's typed; a resize wraps it again.
//!
//! The arrows, Home and End (or Ctrl+A and Ctrl+E), Page Up and Page Down,
//! and Ctrl with the arrows (a word at a time, or the top and the end) move
//! about it. Backspace and Delete take letters out, Enter starts a new
//! line, and Tab goes on to the next fourth column. Ctrl+K cuts the line
//! the cursor's on (pressed again, the next one with it) and Ctrl+U pastes
//! what was cut above the cursor's line, as nano does; Ctrl+Z undoes a
//! change and Ctrl+Y does it again. Ctrl+P (or F2) shows what's written as
//! its readers will see it, as the board says (`Draft::preview`), else
//! wrapped to the window; the arrows and the page keys scroll it, and any
//! other key goes back to writing. Ctrl+S saves; Esc leaves, asking first
//! whether to save where anything changed.
//!
//! Where the board draws what's written as it's written (`Draft::live`: the
//! bulletin typeset, for a caller in Flow), a pane beside the text shows it,
//! or under it in a narrower window, drawn again each time the caller stops
//! typing a moment.
//!
//! Only printable letters are kept, as `ask` keeps them, so nothing written
//! can carry an escape onto another caller's screen. Only rows that
//! changed are drawn again, and nothing while more keys wait (a paste), so
//! a call over a slow line keeps up.

use std::io;
use std::time::Duration;

use flow_sdk::picture::{self, Image};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::call::{Call, Heard, Shown};
use crate::input::{keeps, printable};
use crate::key::Key;
use crate::screen::{deleting, fit, png_size, Screen, CELL};

/// What the editor is given: what's being written, a line saying what to
/// write, the text to start from, how long it may grow, and how what's
/// written looks to its readers.
#[derive(Clone, Copy)]
pub struct Draft<'a> {
    /// Across the top: "The bulletin", "Your reply to …".
    pub title: &'a str,
    /// Under it, dim, where it isn't empty: what to write.
    pub hint: &'a str,
    /// The text to start from.
    pub text: &'a str,
    /// The most lines it may have, and the most bytes (UTF-8, the newline
    /// between two lines counted).
    pub max_lines: usize,
    pub max_bytes: usize,
    /// What a preview shows of the text as it would be saved, as the board
    /// shows it to its readers; where there's none, the text wrapped to the
    /// window.
    pub preview: Option<Previewer<'a>>,
    /// What a pane beside the text shows of it as it's written, as the board
    /// draws it (the bulletin typeset as it's typed, for a caller in Flow);
    /// where there's none, or the window hasn't the room, there's no pane.
    pub live: Option<LiveView<'a>>,
}

impl<'a> Draft<'a> {
    /// A draft titled `title`, empty, of at most 1,000 lines and 64 KB, its
    /// preview the text wrapped to the window.
    pub fn new(title: &'a str) -> Draft<'a> {
        Draft { title, hint: "", text: "", max_lines: 1000, max_bytes: 64 * 1024, preview: None, live: None }
    }
}

/// How a board shows what's written, given the call (is it Flow, how big is
/// its window) and the text as it would be saved.
pub type Previewer<'a> = &'a dyn Fn(&Call, &str) -> Preview;

/// What a preview shows: the text as its readers will see it.
pub enum Preview {
    /// Lines (the sixteen colors allowed, each no wider than the window
    /// less a cell's margin), scrolled with the arrows and the page keys.
    Lines(Vec<String>),
    /// A screen (a picture of it, typeset, say), shown as `Call::show`
    /// shows one, and lines under it.
    Screen(Screen, Vec<String>),
}

/// How a board draws what's written for the pane beside it, given the call
/// and the text as it would be saved.
pub type LiveView<'a> = &'a dyn Fn(&Call, &str) -> Live;

/// What the pane beside the text shows.
pub enum Live {
    /// A picture: a PNG, `rows` rows tall in the caller's cells as a
    /// screen's `{rows N}` makes one, made smaller where the pane is.
    Picture { png: Vec<u8>, rows: u16 },
    /// Words, where there's no picture to show: dim, wrapped to the pane.
    Words(String),
}

/// What the editor says along the bottom, so a caller knows the keys.
pub const EDIT_KEYS: &str = "Ctrl+S saves · Ctrl+P previews · Esc leaves · Ctrl+K cuts · Ctrl+U pastes";

/// What a preview says along the bottom: how it scrolls, and how to go back.
const SCROLLING: &str = "↑ ↓ PgUp PgDn scroll · any other key goes back to writing";
const BACK: &str = "Any key goes back to writing";

/// What it asks when a caller leaves with changes.
const LEAVING: &str = "Save it? Y saves · N throws it away · Esc goes back to it";

const LONGEST: &str = "That's as long as it can be.";
const MOST_LINES: &str = "That's as many lines as it can have.";
const NOTHING: &str = "There's nothing to save yet: Esc leaves.";
const NOTHING_TO_SEE: &str = "There's nothing to preview yet.";

/// The bar's colors (the menus' bar's), a note's, and the keys'.
const BAR: &str = "\x1b[7;36m";
const NOTE: &str = "\x1b[1;33m";
const DIM: &str = "\x1b[2m";
const PLAIN: &str = "\x1b[0m";

/// The pane: beside the text in a window this wide, taking half of it up to
/// this many cells, else under it where the text and it get this many rows
/// each; drawn again once keys stop this long, under this title.
const BESIDE: usize = 96;
const PANE_WIDEST: usize = 82;
const PANE_FEWEST: usize = 8;
const LIVE_WAIT: Duration = Duration::from_millis(200);
const LIVE: &str = "As its readers will see it";

/// A tab goes on to the next of these columns.
const TAB: usize = 4;
/// The changes kept to undo.
const UNDOS: usize = 200;

impl Call {
    /// Lets the caller write, full-screen, starting from `draft.text`: what
    /// they saved (each line without the spaces at its end, no blank lines
    /// before or after it), or `None` where they left without saving or
    /// hung up, which `hung_up` tells apart. The screen is left clear.
    pub fn edit(&mut self, draft: &Draft) -> io::Result<Option<String>> {
        let mut editor = Editor::new(draft);
        let mut all = true;
        // What the pane shows: the text it was drawn for, and where.
        let mut paned: Option<(String, Pane)> = None;
        loop {
            let (cols, rows) = self.window();
            // Drawn once the keys that came together are all in.
            if all || self.pending.is_empty() {
                if all {
                    self.forget()?;
                    paned = None;
                }
                let out = editor.draw(cols, rows, all);
                self.send(&out)?;
                all = false;
            }
            // The pane, drawn again for what's written once the keys stop.
            let wanted = if self.pending.is_empty() { draft.live.zip(editor.layout(cols, rows).pane) } else { None };
            let key = match wanted.map(|(live, pane)| (live, (editor.text.saved(), pane))) {
                Some((live, now)) if paned.as_ref() != Some(&now) => match self.key_within(LIVE_WAIT)? {
                    Heard::Key(key) => key,
                    Heard::HungUp => return Ok(None),
                    Heard::Quiet => {
                        self.pane(live, &now.0, now.1, editor.cursor)?;
                        paned = Some(now);
                        continue;
                    }
                },
                _ => match self.key()? {
                    Some(key) => key,
                    None => return Ok(None),
                },
            };
            editor.note = None;
            let act = if std::mem::take(&mut editor.asking) {
                match key {
                    Key::Char('y' | 'Y') | Key::Ctrl('s') => Act::Save,
                    Key::Char('n' | 'N') => Act::Leave,
                    Key::Resize => {
                        editor.asking = true;
                        Act::Redraw
                    }
                    _ => Act::Nothing,
                }
            } else {
                match key {
                    Key::Ctrl('s') => Act::Save,
                    Key::Esc | Key::Ctrl('c' | 'd' | 'x') if editor.changed() => {
                        editor.asking = true;
                        Act::Nothing
                    }
                    Key::Esc | Key::Ctrl('c' | 'd' | 'x') => Act::Leave,
                    Key::Resize | Key::Ctrl('l') => Act::Redraw,
                    Key::Ctrl('p') => Act::Preview,
                    Key::Other(seq) if seq == b"\x1bOQ" || seq == b"\x1b[12~" => Act::Preview,
                    key => Act::Press(key),
                }
            };
            match act {
                Act::Save => {
                    let text = editor.text.saved();
                    if text.is_empty() {
                        editor.note = Some(NOTHING);
                    } else {
                        self.forget()?;
                        self.send(&format!("{PLAIN}\x1b[2J\x1b[H\x1b[?25h"))?;
                        return Ok(Some(text));
                    }
                }
                Act::Leave => {
                    self.forget()?;
                    self.send(&format!("{PLAIN}\x1b[2J\x1b[H\x1b[?25h"))?;
                    return Ok(None);
                }
                Act::Redraw => all = true,
                Act::Preview => {
                    let text = editor.text.saved();
                    if text.is_empty() {
                        editor.note = Some(NOTHING_TO_SEE);
                    } else {
                        if !self.preview(draft, &editor.title, &text)? {
                            return Ok(None);
                        }
                        all = true;
                    }
                }
                Act::Nothing => {}
                Act::Press(key) => {
                    let (cols, rows) = self.window();
                    let Layout { width, height, .. } = editor.layout(cols, rows);
                    editor.note = editor.text.press(&key, width, height.saturating_sub(1).max(1));
                }
            }
        }
    }

    /// Shows `text` as `draft` says its readers will see it, until a key
    /// other than one that scrolls it: `false` where the caller hung up.
    fn preview(&mut self, draft: &Draft, title: &str, text: &str) -> io::Result<bool> {
        let shown = match draft.preview {
            Some(preview) => preview(self, text),
            None => Preview::Lines(wrapped(text, self.window().0.saturating_sub(2).max(10))),
        };
        let mut top = 0;
        loop {
            let (cols, rows) = self.window();
            let height = rows.saturating_sub(2).max(1);
            match &shown {
                Preview::Screen(screen, lines) => {
                    self.show_leaving(screen, &[], lines.len() as u16 + 2)?;
                    for line in lines {
                        self.say(&format!(" {line}\n"))?;
                    }
                    self.send(&format!("\x1b[?25l\x1b[{rows};1H {DIM}{}{PLAIN}", cut(BACK, cols.saturating_sub(2))))?;
                }
                Preview::Lines(lines) => self.send(&page(title, lines, top, cols, rows))?,
            }
            let most = match &shown {
                Preview::Lines(lines) => lines.len().saturating_sub(height),
                Preview::Screen(..) => 0,
            };
            // The keys that scroll, where there's more than a window of it;
            // a resize draws it again; any other key goes back.
            loop {
                let Some(key) = self.key()? else { return Ok(false) };
                let to = match key {
                    Key::Resize => break,
                    Key::Up => top.saturating_sub(1),
                    Key::Down => top + 1,
                    Key::PageUp => top.saturating_sub(height.saturating_sub(1).max(1)),
                    Key::PageDown => top + height.saturating_sub(1).max(1),
                    Key::Home => 0,
                    Key::End => most,
                    _ => return Ok(true),
                };
                if matches!(shown, Preview::Lines(_)) && to.min(most) != top {
                    top = to.min(most);
                    break;
                }
            }
        }
    }

    /// Draws the pane at `pane`: its title, and under it what `live` makes of
    /// `text`. The picture it showed before goes once the new one's in, so
    /// it's never empty between them; then the cursor goes back to `cursor`.
    fn pane(&mut self, live: LiveView, text: &str, pane: Pane, (row, col): (usize, usize)) -> io::Result<()> {
        let mut out = format!("\x1b[?25l{PLAIN}");
        for r in pane.row..pane.row + pane.rows {
            out.push_str(&format!("\x1b[{};{}H\x1b[K", r + 1, pane.col + 1));
        }
        let title = if pane.beside {
            cut(LIVE, pane.cols)
        } else {
            let title = cut(&format!("── {LIVE} "), pane.cols);
            format!("{title}{}", "─".repeat(pane.cols.saturating_sub(title.width())))
        };
        out.push_str(&format!("\x1b[{};{}H{DIM}{title}{PLAIN}", pane.row + 1, pane.col + 1));
        let (top, cols, rows) = (pane.row + 1, pane.cols.max(1), pane.rows.saturating_sub(1));
        let old = std::mem::take(&mut self.shown);
        let shown = if text.is_empty() { Live::Words(NOTHING_TO_SEE.to_string()) } else { live(self, text) };
        match shown {
            Live::Picture { png, rows: tall } => {
                if let Some(size) = png_size(&png).filter(|_| self.is_flow() && rows > 0) {
                    let cell = self.size().cell().filter(|&(w, h)| w > 0 && h > 0).unwrap_or(CELL);
                    let (pc, pr) = fit(size, cell, Some(tall).filter(|&t| t > 0), cols as u16, rows as u16);
                    let left = pane.col + cols.saturating_sub(pc as usize) / 2;
                    let id = self.next_id();
                    out.push_str(&picture::upload(id, &Image::png(png), 1, pc, pr));
                    out.push_str(&picture::placeholders(id, 1, (left as u16, top as u16), pc, pr));
                    self.shown.push(Shown::Picture(id));
                }
            }
            Live::Words(words) => {
                for (i, line) in wrapped(&printable(&words), cols).iter().take(rows).enumerate() {
                    out.push_str(&format!("\x1b[{};{}H{DIM}{line}{PLAIN}", top + i + 1, pane.col + 1));
                }
            }
        }
        out.push_str(&deleting(old));
        out.push_str(&format!("\x1b[{};{}H\x1b[?25h", row + 1, col + 1));
        self.send(&out)
    }

    /// The caller's window, or 80 × 24 where it's unsaid.
    fn window(&self) -> (usize, usize) {
        let size = self.size();
        (if size.cols == 0 { 80 } else { size.cols as usize }, if size.rows == 0 { 24 } else { size.rows as usize })
    }
}

/// What a key comes to in the editor's loop.
enum Act {
    Save,
    Leave,
    Redraw,
    Preview,
    Nothing,
    Press(Key),
}

/// `text`'s lines wrapped to `width` cells, at words: a preview where the
/// board gives none.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let letters: Vec<char> = line.chars().collect();
        out.extend(wrap(&letters, width).into_iter().map(|(start, end)| clip(&letters[start..end], width)));
    }
    out
}

/// A preview of `lines` under a bar titled `title`, from line `top`, in a
/// window of `cols` × `rows`: the whole window, drawn afresh, the cursor
/// hidden.
fn page(title: &str, lines: &[String], top: usize, cols: usize, rows: usize) -> String {
    let height = rows.saturating_sub(2).max(1);
    let across = cols.saturating_sub(1).max(10);
    let place = if lines.len() > height { format!("lines {}–{} of {} ", top + 1, (top + height).min(lines.len()), lines.len()) } else { String::new() };
    let title = cut(&format!(" Preview: {title}"), across.saturating_sub(place.width() + 1).max(across / 2));
    let place = if title.width() + place.width() <= across { place } else { String::new() };
    let fill = " ".repeat(across.saturating_sub(title.width() + place.width()));
    let mut out = format!("\x1b[?25l{PLAIN}\x1b[2J\x1b[1;1H{BAR}\x1b[1m{title}\x1b[22m{fill}{place}{PLAIN}");
    for (i, line) in lines.iter().skip(top).take(height).enumerate() {
        out.push_str(&format!("\x1b[{};1H {line}{PLAIN}", i + 2));
    }
    let keys = if lines.len() > height { SCROLLING } else { BACK };
    out.push_str(&format!("\x1b[{};1H {DIM}{}{PLAIN}", height + 2, cut(keys, across - 1)));
    out
}

/// The editor in a window: the text, the row it's scrolled to, and what the
/// caller's screen shows now, a row at a time.
pub(crate) struct Editor {
    text: Text,
    title: String,
    hint: String,
    /// The text it started from, to know whether anything changed.
    start: String,
    /// The first of the text's rows in the window.
    top: usize,
    /// The window's rows as they were last drawn.
    drawn: Vec<String>,
    /// Said along the bottom in place of the keys, until the next key.
    note: Option<&'static str>,
    /// Asking whether to save, on the way out.
    asking: bool,
    /// Whether the board draws a pane of what's written, and where the
    /// cursor was last put (a row and a column, from 0).
    live: bool,
    cursor: (usize, usize),
}

/// Where the editor's parts go in a window: the text's room (its width in
/// cells, a cell's margin to its left and the cell after its last left for
/// the cursor; its height in rows), and the pane's, where there's one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    width: usize,
    height: usize,
    pane: Option<Pane>,
}

/// The pane's place: its first row and column (from 0), its size in cells,
/// and whether it's beside the text or under it. It runs to the window's
/// edge but for the last column, so erasing to a row's end clears its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pane {
    row: usize,
    col: usize,
    cols: usize,
    rows: usize,
    beside: bool,
}

impl Editor {
    pub(crate) fn new(draft: &Draft) -> Editor {
        let text = Text::new(draft.text, draft.max_lines, draft.max_bytes);
        let start = text.text();
        let one_line = |s: &str| printable(s).replace('\n', " ");
        Editor { text, title: one_line(draft.title), hint: one_line(draft.hint), start, top: 0, drawn: Vec::new(), note: None, asking: false, live: draft.live.is_some(), cursor: (0, 0) }
    }

    fn changed(&self) -> bool {
        self.text.text() != self.start
    }

    /// Where things go in a window of `cols` × `rows`: the text between the
    /// bar (and the hint) and the keys, and where the board draws a pane of
    /// it and there's room, the pane beside it (half a wide window, a rule
    /// between them) or under it.
    fn layout(&self, cols: usize, rows: usize) -> Layout {
        let first = if self.hint.is_empty() { 1 } else { 2 };
        let total = rows.saturating_sub(first + 1).max(1);
        let full = Layout { width: cols.saturating_sub(2).max(10), height: total, pane: None };
        if !self.live || total < PANE_FEWEST {
            return full;
        }
        if cols >= BESIDE {
            let rule = cols - (cols / 2).min(PANE_WIDEST);
            return Layout { width: rule - 2, height: total, pane: Some(Pane { row: first, col: rule + 2, cols: cols - rule - 3, rows: total, beside: true }) };
        }
        if total >= 2 * PANE_FEWEST {
            let under = total / 2;
            return Layout { width: full.width, height: total - under, pane: Some(Pane { row: first + total - under, col: 1, cols: cols.saturating_sub(2), rows: under, beside: false }) };
        }
        full
    }

    /// The window's rows as they should be, and where the cursor goes in it
    /// (a row and a column, from 0), scrolled to keep the cursor in sight.
    fn screen(&mut self, cols: usize, rows: usize) -> (Vec<String>, (usize, usize)) {
        let Layout { width, height, pane } = self.layout(cols, rows);
        let shown = self.text.rows(width);
        let (r, col) = self.text.cursor(&shown);
        // Back up where the text got shorter, then on to the cursor.
        self.top = self.top.min(shown.len().saturating_sub(height));
        if r < self.top {
            self.top = r;
        } else if r >= self.top + height {
            self.top = r + 1 - height;
        }
        let across = cols.saturating_sub(1).max(10);
        let (line, at) = self.text.at;
        let place = format!("line {} of {} · column {} ", line + 1, self.text.lines.len(), at + 1);
        let title = cut(&format!(" {}", self.title), across.saturating_sub(place.width() + 1).max(across / 2));
        let place = if title.width() + place.width() <= across { place } else { String::new() };
        let fill = " ".repeat(across.saturating_sub(title.width() + place.width()));
        // Each row as it's drawn, erasing what's after it to the row's end
        // but beside the pane, which it leaves alone.
        let mut out = vec![format!("{BAR}\x1b[1m{title}\x1b[22m{fill}{place}{PLAIN}\x1b[K")];
        if !self.hint.is_empty() {
            out.push(format!(" {DIM}{}{PLAIN}\x1b[K", cut(&self.hint, across - 1)));
        }
        let first = out.len();
        for i in self.top..self.top + height {
            let letters = match shown.get(i) {
                Some(row) => clip(&self.text.lines[row.line][row.start..row.end], width),
                None => String::new(),
            };
            out.push(match pane {
                Some(p) if p.beside => format!(" {letters}{}{DIM}│{PLAIN}", " ".repeat((p.col - 3).saturating_sub(letters.width()))),
                _ => format!(" {letters}{PLAIN}\x1b[K"),
            });
        }
        // The rows of a pane under the text are the pane's to draw.
        if let Some(p) = pane.filter(|p| !p.beside) {
            out.extend(std::iter::repeat_n(String::new(), p.rows));
        }
        let cursor = if self.asking {
            out.push(format!(" {NOTE}{}{PLAIN}\x1b[K", cut(LEAVING, across - 1)));
            (out.len() - 1, (1 + LEAVING.width()).min(across))
        } else {
            match self.note {
                Some(note) => out.push(format!(" {NOTE}{}{PLAIN}\x1b[K", cut(note, across - 1))),
                None => out.push(format!(" {DIM}{}{PLAIN}\x1b[K", cut(EDIT_KEYS, across - 1))),
            }
            (first + r - self.top, 1 + col.min(width))
        };
        (out, cursor)
    }

    /// What brings the caller's screen to what it should be: the rows that
    /// changed since it was last drawn (all of them after `all`), with the
    /// cursor hidden meanwhile and put back after.
    pub(crate) fn draw(&mut self, cols: usize, rows: usize, all: bool) -> String {
        let (screen, (row, col)) = self.screen(cols, rows);
        let mut out = String::from("\x1b[?25l");
        if all {
            out.push_str(&format!("{PLAIN}\x1b[2J"));
            self.drawn.clear();
        }
        for (i, line) in screen.iter().enumerate() {
            // An empty row is the pane's to draw.
            if self.drawn.get(i) != Some(line) && !line.is_empty() {
                out.push_str(&format!("\x1b[{};1H{line}", i + 1));
            }
        }
        self.drawn = screen;
        self.cursor = (row, col);
        out.push_str(&format!("\x1b[{};{}H\x1b[?25h", row + 1, col + 1));
        out
    }
}

/// `text` cut to `room` cells, an ellipsis saying where.
fn cut(text: &str, room: usize) -> String {
    if text.width() <= room {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > room {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// As many of `letters` as fit in `width` cells: a row's spaces that hang
/// past its end aren't drawn.
fn clip(letters: &[char], width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for &c in letters {
        used += cells(c);
        if used > width {
            break;
        }
        out.push(c);
    }
    out
}

fn cells(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// Where `letters` break into rows of `width` cells, as the ranges of them
/// each row shows: at spaces where they can, a word too long for a row
/// broken where it must be. Spaces at a row's end hang past it, so they
/// never start the next.
fn wrap(letters: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut start = 0;
    while start < letters.len() {
        let (mut i, mut used, mut space) = (start, 0, None);
        while i < letters.len() {
            let c = letters[i];
            if c == ' ' {
                used += 1;
                i += 1;
                space = Some(i);
                continue;
            }
            if used + cells(c) > width {
                break;
            }
            used += cells(c);
            i += 1;
        }
        if i == letters.len() {
            rows.push((start, i));
            break;
        }
        let end = match space {
            Some(s) if s > start => s,
            _ => i.max(start + 1),
        };
        rows.push((start, end));
        start = end;
    }
    if rows.is_empty() {
        rows.push((0, 0));
    }
    rows
}

/// A row of the text as it's shown: line `line`'s letters `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    line: usize,
    start: usize,
    end: usize,
}

/// The text as it was, to undo to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snap {
    lines: Vec<Vec<char>>,
    at: (usize, usize),
}

/// The kinds of change undone a run at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Typing,
    Deleting,
    Other,
}

/// The text being written, the cursor in it, and what can be undone: all
/// that doesn't need the window, so it's tested without one.
#[derive(Debug, Clone)]
pub(crate) struct Text {
    lines: Vec<Vec<char>>,
    /// The cursor: a line, and the letter in it the cursor is before.
    at: (usize, usize),
    /// The column Up and Down keep to, from where they started.
    goal: Option<usize>,
    /// The lines Ctrl+K cut, and whether the last key cut (so the next cut
    /// adds to them).
    cut: Vec<Vec<char>>,
    cutting: bool,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// The last change, and where it left the cursor: typing on from there
    /// is the same change, undone with it.
    last: Option<(Change, (usize, usize))>,
    max_lines: usize,
    max_bytes: usize,
}

impl Text {
    pub(crate) fn new(text: &str, max_lines: usize, max_bytes: usize) -> Text {
        let lines = printable(text).split('\n').map(|l| l.chars().collect()).collect();
        Text { lines, at: (0, 0), goal: None, cut: Vec::new(), cutting: false, undo: Vec::new(), redo: Vec::new(), last: None, max_lines, max_bytes }
    }

    /// The text as it is, its lines joined by newlines.
    pub(crate) fn text(&self) -> String {
        self.lines.iter().map(|l| l.iter().collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// The text to keep: each line without the spaces at its end, and no
    /// blank lines before or after it.
    pub(crate) fn saved(&self) -> String {
        let lines: Vec<String> = self.lines.iter().map(|l| l.iter().collect::<String>().trim_end().to_string()).collect();
        let Some(first) = lines.iter().position(|l| !l.is_empty()) else { return String::new() };
        let last = lines.iter().rposition(|l| !l.is_empty()).unwrap_or(first);
        lines[first..=last].join("\n")
    }

    fn bytes(&self) -> usize {
        self.lines.iter().map(|l| l.iter().map(|c| c.len_utf8()).sum::<usize>()).sum::<usize>() + self.lines.len() - 1
    }

    /// The text's rows at `width` cells.
    fn rows(&self, width: usize) -> Vec<Row> {
        self.lines.iter().enumerate().flat_map(|(line, l)| wrap(l, width).into_iter().map(move |(start, end)| Row { line, start, end })).collect()
    }

    /// The row the cursor's on among `rows`, and its column in cells. A
    /// cursor at the end of a row that goes on in the next is at the start
    /// of the next.
    fn cursor(&self, rows: &[Row]) -> (usize, usize) {
        let (line, at) = self.at;
        let r = rows.iter().rposition(|row| row.line == line && row.start <= at).unwrap_or(0);
        (r, self.lines[line][rows[r].start..at].iter().map(|&c| cells(c)).sum())
    }

    /// The cursor onto row `r`, as near column `goal` as its letters allow:
    /// on a row the line goes on from, before its last letter, since after
    /// it is the next row's start.
    fn onto(&mut self, rows: &[Row], r: usize, goal: usize) {
        let row = rows[r];
        let goes_on = rows.get(r + 1).is_some_and(|next| next.line == row.line);
        let most = if goes_on { row.end - 1 } else { row.end };
        let letters = &self.lines[row.line];
        let (mut at, mut col) = (row.start, 0);
        while at < most && col + cells(letters[at]) <= goal {
            col += cells(letters[at]);
            at += 1;
        }
        self.at = (row.line, at);
    }

    /// What `key` does, the text `width` cells wide and a page `page` rows:
    /// a note to say where the key was refused.
    pub(crate) fn press(&mut self, key: &Key, width: usize, page: usize) -> Option<&'static str> {
        let cutting = std::mem::take(&mut self.cutting);
        let goal = self.goal.take();
        let line_end = |t: &Text, line: usize| (line, t.lines[line].len());
        match key {
            Key::Char(c) if keeps(*c) => return self.insert(&[*c]),
            Key::Tab => {
                let (line, at) = self.at;
                let col: usize = self.lines[line][..at].iter().map(|&c| cells(c)).sum();
                return self.insert(&vec![' '; TAB - col % TAB]);
            }
            Key::Enter => return self.enter(),
            Key::Backspace => self.backspace(),
            Key::Delete => self.delete(),
            Key::Left => {
                let (line, at) = self.at;
                if at > 0 {
                    self.at.1 -= 1;
                } else if line > 0 {
                    self.at = line_end(self, line - 1);
                }
            }
            Key::Right => {
                let (line, at) = self.at;
                if at < self.lines[line].len() {
                    self.at.1 += 1;
                } else if line + 1 < self.lines.len() {
                    self.at = (line + 1, 0);
                }
            }
            Key::Up => self.vertical(width, -1, goal),
            Key::Down => self.vertical(width, 1, goal),
            Key::PageUp => self.vertical(width, -(page as isize), goal),
            Key::PageDown => self.vertical(width, page as isize, goal),
            Key::Home | Key::Ctrl('a') => {
                let rows = self.rows(width);
                let (r, _) = self.cursor(&rows);
                self.at = (rows[r].line, rows[r].start);
            }
            Key::End | Key::Ctrl('e') => {
                let rows = self.rows(width);
                let (r, _) = self.cursor(&rows);
                self.onto(&rows, r, usize::MAX);
            }
            Key::Ctrl('k') => self.cut_line(cutting),
            Key::Ctrl('u') => return self.paste(),
            Key::Ctrl('z') => self.undo(),
            Key::Ctrl('y') => self.redo(),
            Key::Other(seq) => match seq.as_slice() {
                b"\x1b[1;5D" => self.word_left(),
                b"\x1b[1;5C" => self.word_right(),
                b"\x1b[1;5H" => self.at = (0, 0),
                b"\x1b[1;5F" => self.at = line_end(self, self.lines.len() - 1),
                _ => {}
            },
            _ => {}
        }
        None
    }

    /// Up or down `by` rows, keeping to the column the moving started from:
    /// past the first row to the start, past the last to the end.
    fn vertical(&mut self, width: usize, by: isize, goal: Option<usize>) {
        let rows = self.rows(width);
        let (r, col) = self.cursor(&rows);
        let goal = goal.unwrap_or(col);
        let last = rows.len() - 1;
        let to = r as isize + by;
        if to < 0 {
            if r == 0 {
                self.at = (0, 0);
            } else {
                self.onto(&rows, 0, goal);
            }
        } else if to as usize > last {
            if r == last {
                let line = self.lines.len() - 1;
                self.at = (line, self.lines[line].len());
            } else {
                self.onto(&rows, last, goal);
            }
        } else {
            self.onto(&rows, to as usize, goal);
        }
        self.goal = Some(goal);
    }

    fn word_left(&mut self) {
        let (line, mut at) = self.at;
        if at == 0 {
            if line > 0 {
                self.at = (line - 1, self.lines[line - 1].len());
            }
            return;
        }
        let l = &self.lines[line];
        while at > 0 && l[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !l[at - 1].is_whitespace() {
            at -= 1;
        }
        self.at = (line, at);
    }

    fn word_right(&mut self) {
        let (line, mut at) = self.at;
        let l = &self.lines[line];
        if at == l.len() {
            if line + 1 < self.lines.len() {
                self.at = (line + 1, 0);
            }
            return;
        }
        while at < l.len() && !l[at].is_whitespace() {
            at += 1;
        }
        while at < l.len() && l[at].is_whitespace() {
            at += 1;
        }
        self.at = (line, at);
    }

    /// Before a change: the text as it is, kept to undo to, unless this goes
    /// on from the last change of its kind (typing on, deleting on).
    fn change(&mut self, kind: Change) {
        let goes_on = kind != Change::Other && self.last == Some((kind, self.at));
        if !goes_on {
            self.undo.push(Snap { lines: self.lines.clone(), at: self.at });
            if self.undo.len() > UNDOS {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }

    /// After a change: where it left the cursor, for the next to go on from.
    fn changed(&mut self, kind: Change) {
        self.last = Some((kind, self.at));
    }

    fn insert(&mut self, letters: &[char]) -> Option<&'static str> {
        let more: usize = letters.iter().map(|c| c.len_utf8()).sum();
        if self.bytes() + more > self.max_bytes {
            return Some(LONGEST);
        }
        self.change(Change::Typing);
        let (line, at) = self.at;
        self.lines[line].splice(at..at, letters.iter().copied());
        self.at.1 += letters.len();
        self.changed(Change::Typing);
        None
    }

    fn enter(&mut self) -> Option<&'static str> {
        if self.lines.len() >= self.max_lines {
            return Some(MOST_LINES);
        }
        if self.bytes() + 1 > self.max_bytes {
            return Some(LONGEST);
        }
        self.change(Change::Other);
        let (line, at) = self.at;
        let rest = self.lines[line].split_off(at);
        self.lines.insert(line + 1, rest);
        self.at = (line + 1, 0);
        self.changed(Change::Other);
        None
    }

    fn backspace(&mut self) {
        let (line, at) = self.at;
        if (line, at) == (0, 0) {
            return;
        }
        self.change(Change::Deleting);
        if at > 0 {
            self.lines[line].remove(at - 1);
            self.at.1 -= 1;
        } else {
            let rest = self.lines.remove(line);
            self.at = (line - 1, self.lines[line - 1].len());
            self.lines[line - 1].extend(rest);
        }
        self.changed(Change::Deleting);
    }

    fn delete(&mut self) {
        let (line, at) = self.at;
        if at == self.lines[line].len() && line + 1 == self.lines.len() {
            return;
        }
        self.change(Change::Deleting);
        if at < self.lines[line].len() {
            self.lines[line].remove(at);
        } else {
            let next = self.lines.remove(line + 1);
            self.lines[line].extend(next);
        }
        self.changed(Change::Deleting);
    }

    /// Cuts the cursor's line, after the lines cut just before (`again`).
    fn cut_line(&mut self, again: bool) {
        let line = self.at.0;
        if self.lines.len() == 1 && self.lines[0].is_empty() {
            self.cutting = again;
            return;
        }
        self.change(Change::Other);
        let taken = if self.lines.len() == 1 { std::mem::take(&mut self.lines[0]) } else { self.lines.remove(line) };
        if !again {
            self.cut.clear();
        }
        self.cut.push(taken);
        self.at = (line.min(self.lines.len() - 1), 0);
        self.cutting = true;
        self.changed(Change::Other);
    }

    /// The lines cut, above the cursor's line, the cursor left where it was.
    fn paste(&mut self) -> Option<&'static str> {
        if self.cut.is_empty() {
            return None;
        }
        let more: usize = self.cut.iter().map(|l| l.iter().map(|c| c.len_utf8()).sum::<usize>() + 1).sum();
        if self.lines.len() + self.cut.len() > self.max_lines {
            return Some(MOST_LINES);
        }
        if self.bytes() + more > self.max_bytes {
            return Some(LONGEST);
        }
        self.change(Change::Other);
        let line = self.at.0;
        let n = self.cut.len();
        self.lines.splice(line..line, self.cut.iter().cloned());
        self.at = (line + n, 0);
        self.changed(Change::Other);
        None
    }

    fn undo(&mut self) {
        if let Some(snap) = self.undo.pop() {
            self.redo.push(Snap { lines: std::mem::replace(&mut self.lines, snap.lines), at: self.at });
            self.at = snap.at;
            self.last = None;
        }
    }

    fn redo(&mut self) {
        if let Some(snap) = self.redo.pop() {
            self.undo.push(Snap { lines: std::mem::replace(&mut self.lines, snap.lines), at: self.at });
            self.at = snap.at;
            self.last = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A text with `typed` typed into it, each letter a key, `\n` Enter.
    fn typed(typed: &str, width: usize) -> Text {
        let mut t = Text::new("", 1000, 64 * 1024);
        press(&mut t, typed, width);
        t
    }

    fn press(t: &mut Text, typed: &str, width: usize) {
        for c in typed.chars() {
            let key = if c == '\n' { Key::Enter } else { Key::Char(c) };
            assert_eq!(t.press(&key, width, 5), None);
        }
    }

    fn keys(t: &mut Text, keys: &[Key], width: usize) {
        for k in keys {
            t.press(k, width, 5);
        }
    }

    fn shown(t: &Text, width: usize) -> Vec<String> {
        t.rows(width).iter().map(|r| t.lines[r.line][r.start..r.end].iter().collect()).collect()
    }

    #[test]
    fn text_wraps_at_spaces_its_end_spaces_hanging_and_a_word_too_long_broken() {
        let t = Text::new("the quick brown fox jumps\n\nabcdefghijklmnop", 100, 1000);
        assert_eq!(shown(&t, 10), ["the quick ", "brown fox ", "jumps", "", "abcdefghij", "klmnop"]);
        let t = Text::new("浦和レッズの試合", 100, 1000);
        assert_eq!(shown(&t, 10), ["浦和レッズ", "の試合"], "wide letters take two cells");
        assert_eq!(clip(&"brown fox   ".chars().collect::<Vec<_>>(), 10), "brown fox ", "the spaces past the edge aren't drawn");
    }

    #[test]
    fn the_cursor_at_a_rows_end_is_at_the_next_rows_start_and_up_and_down_keep_their_column() {
        let mut t = Text::new("the quick brown fox jumps", 100, 1000);
        let rows = t.rows(10);
        t.at = (0, 10);
        assert_eq!(t.cursor(&rows), (1, 0), "after 'quick ', the start of 'brown'");
        t.at = (0, 7);
        keys(&mut t, &[Key::Down], 10);
        assert_eq!(t.at, (0, 17), "'brown fox': the same column");
        keys(&mut t, &[Key::Down], 10);
        assert_eq!(t.at, (0, 25), "'jumps' is shorter: its end");
        keys(&mut t, &[Key::Up], 10);
        assert_eq!(t.at, (0, 17), "back to the column it started from");
        keys(&mut t, &[Key::Up, Key::Up], 10);
        assert_eq!(t.at, (0, 0), "up from the first row: its start");
        keys(&mut t, &[Key::End], 10);
        assert_eq!(t.at, (0, 9), "the end of a row that goes on stops before its last letter");
        keys(&mut t, &[Key::Ctrl('e'), Key::PageDown], 10);
        assert_eq!(t.at, (0, 25), "a page down past the last row: the end");
        keys(&mut t, &[Key::Home], 10);
        assert_eq!(t.at, (0, 20));
    }

    #[test]
    fn typing_enter_backspace_and_delete_split_and_join_lines() {
        let mut t = typed("hello world", 40);
        keys(&mut t, &[Key::Left, Key::Left, Key::Left, Key::Left, Key::Left, Key::Backspace, Key::Enter], 40);
        assert_eq!(t.text(), "hello\nworld");
        assert_eq!(t.at, (1, 0));
        keys(&mut t, &[Key::Backspace], 40);
        assert_eq!((t.text().as_str(), t.at), ("helloworld", (0, 5)), "backspace at a line's start joins it to the one before");
        keys(&mut t, &[Key::Enter, Key::Up, Key::End, Key::Delete], 40);
        assert_eq!(t.text(), "helloworld", "delete at a line's end joins the next to it");
        keys(&mut t, &[Key::Ctrl('a'), Key::Tab], 40);
        assert_eq!((t.text().as_str(), t.at), ("    helloworld", (0, 4)), "a tab to the next fourth column");
        keys(&mut t, &[Key::Char('\u{1b}'), Key::Char('\u{202e}')], 40);
        assert_eq!(t.text(), "    helloworld", "nothing a terminal would act on");
        keys(&mut t, &[Key::Left, Key::Left, Key::Right], 40);
        assert_eq!(t.at, (0, 3));
    }

    #[test]
    fn words_left_and_right_and_the_top_and_the_end() {
        let mut t = Text::new("one two  three\nfour", 100, 1000);
        let ctrl = |s: &[u8]| Key::Other(s.to_vec());
        keys(&mut t, &[ctrl(b"\x1b[1;5C")], 40);
        assert_eq!(t.at, (0, 4));
        keys(&mut t, &[ctrl(b"\x1b[1;5C"), ctrl(b"\x1b[1;5C"), ctrl(b"\x1b[1;5C")], 40);
        assert_eq!(t.at, (1, 0), "past the line's end, to the next");
        keys(&mut t, &[ctrl(b"\x1b[1;5D"), ctrl(b"\x1b[1;5D")], 40);
        assert_eq!(t.at, (0, 9), "back to the line before, then to the start of 'three'");
        keys(&mut t, &[ctrl(b"\x1b[1;5F")], 40);
        assert_eq!(t.at, (1, 4));
        keys(&mut t, &[ctrl(b"\x1b[1;5H")], 40);
        assert_eq!(t.at, (0, 0));
    }

    #[test]
    fn cut_lines_go_together_and_are_pasted_above_the_cursors_line() {
        let mut t = Text::new("one\ntwo\nthree\nfour", 100, 1000);
        keys(&mut t, &[Key::Ctrl('k'), Key::Ctrl('k')], 40);
        assert_eq!((t.text().as_str(), t.at), ("three\nfour", (0, 0)));
        keys(&mut t, &[Key::Down, Key::Ctrl('u')], 40);
        assert_eq!((t.text().as_str(), t.at), ("three\none\ntwo\nfour", (3, 0)), "both, above 'four', the cursor still on it");
        keys(&mut t, &[Key::Up, Key::Ctrl('k'), Key::Down, Key::Ctrl('k')], 40);
        assert_eq!(t.cut, [vec!['f', 'o', 'u', 'r']], "a cut after another key starts afresh");
        let mut t = Text::new("all", 100, 1000);
        keys(&mut t, &[Key::Ctrl('k'), Key::Ctrl('k'), Key::Ctrl('u')], 40);
        assert_eq!(t.text(), "all\n", "the last line cut leaves an empty one");
    }

    #[test]
    fn undo_takes_back_a_run_of_typing_at_once_and_redo_puts_it_back() {
        let mut t = typed("hello world", 40);
        keys(&mut t, &[Key::Enter], 40);
        press(&mut t, "again", 40);
        keys(&mut t, &[Key::Backspace, Key::Backspace], 40);
        assert_eq!(t.text(), "hello world\naga");
        keys(&mut t, &[Key::Ctrl('z')], 40);
        assert_eq!(t.text(), "hello world\nagain", "the two backspaces, together");
        keys(&mut t, &[Key::Ctrl('z')], 40);
        assert_eq!((t.text().as_str(), t.at), ("hello world\n", (1, 0)), "the typing, together");
        keys(&mut t, &[Key::Ctrl('z'), Key::Ctrl('z')], 40);
        assert_eq!(t.text(), "", "Enter, then the first typing");
        keys(&mut t, &[Key::Ctrl('z')], 40);
        assert_eq!(t.text(), "", "nothing more to undo");
        keys(&mut t, &[Key::Ctrl('y'), Key::Ctrl('y')], 40);
        assert_eq!((t.text().as_str(), t.at), ("hello world\n", (1, 0)));
        keys(&mut t, &[Key::Char('!'), Key::Ctrl('y')], 40);
        assert_eq!(t.text(), "hello world\n!", "a change after undoing leaves nothing to redo");
        let mut t = typed("ab", 40);
        keys(&mut t, &[Key::Left, Key::Char('x')], 40);
        keys(&mut t, &[Key::Ctrl('z')], 40);
        assert_eq!(t.text(), "ab", "typing somewhere else is a change of its own");
    }

    #[test]
    fn past_its_limits_a_key_is_refused_with_a_note() {
        let mut t = Text::new("abc", 2, 6);
        t.at = (0, 3);
        assert_eq!(t.press(&Key::Enter, 40, 5), None);
        assert_eq!(t.press(&Key::Enter, 40, 5), Some(MOST_LINES));
        assert_eq!(t.press(&Key::Char('d'), 40, 5), None);
        assert_eq!(t.press(&Key::Char('é'), 40, 5), Some(LONGEST), "two bytes, one too many");
        assert_eq!(t.text(), "abc\nd");
    }

    #[test]
    fn what_is_saved_has_no_spaces_at_its_lines_ends_nor_blank_lines_around_it() {
        let t = Text::new("\n\n  first   \n\nsecond  \n\n", 100, 1000);
        assert_eq!(t.saved(), "  first\n\nsecond");
        assert_eq!(Text::new(" \n \n", 100, 1000).saved(), "");
        assert_eq!(Text::new("a\r\nb\tc\x1b[2J", 100, 1000).text(), "a\nb    c[2J", "the start through printable");
    }

    #[test]
    fn the_first_draw_has_the_bar_and_the_keys_and_a_key_draws_only_its_row_again() {
        let mut e = Editor::new(&Draft { hint: "Say hello", text: "first\nsecond", ..Draft::new("The bulletin") });
        let all = e.draw(40, 10, true);
        assert!(all.starts_with("\x1b[?25l\x1b[0m\x1b[2J"), "{all:?}");
        assert!(all.contains("\x1b[1;1H\x1b[7;36m\x1b[1m The bulletin\x1b[22m"), "{all:?}");
        assert!(all.contains("line 1 of 2 · column 1"), "{all:?}");
        assert!(all.contains("\x1b[2;1H \x1b[2mSay hello"), "the hint under the bar: {all:?}");
        assert!(all.contains("\x1b[3;1H first") && all.contains("\x1b[4;1H second"), "{all:?}");
        assert!(all.contains("\x1b[10;1H \x1b[2mCtrl+S saves"), "the keys along the bottom: {all:?}");
        assert!(all.ends_with("\x1b[3;2H\x1b[?25h"), "the cursor at the text's start: {all:?}");
        e.text.press(&Key::Char('!'), 38, 6);
        let one = e.draw(40, 10, false);
        assert!(one.contains("\x1b[3;1H !first") && one.contains("column 2"), "{one:?}");
        assert!(!one.contains("second") && !one.contains("Ctrl+S"), "nothing else drawn again: {one:?}");
        assert!(one.ends_with("\x1b[3;3H\x1b[?25h"));
        assert_eq!(e.draw(40, 10, false), "\x1b[?25l\x1b[3;3H\x1b[?25h", "nothing changed, nothing drawn");
    }

    #[test]
    fn a_long_text_scrolls_to_keep_the_cursor_in_sight() {
        let text: String = (1..=30).map(|n| format!("line {n}\n")).collect();
        let mut e = Editor::new(&Draft { text: &text, ..Draft::new("Long") });
        e.draw(40, 10, true);
        for _ in 0..12 {
            e.text.press(&Key::Down, 38, 7);
        }
        let out = e.draw(40, 10, false);
        assert_eq!(e.top, 5, "line 13 the window's last of eight");
        assert!(out.contains("\x1b[2;1H line 6") && out.ends_with("\x1b[9;2H\x1b[?25h"), "{out:?}");
        let mut e = Editor::new(&Draft { text: "short", ..Draft::new("Short") });
        e.top = 20;
        e.draw(40, 10, true);
        assert_eq!(e.top, 0, "pulled back where the text is shorter than it was");
    }

    #[test]
    fn a_preview_without_the_boards_wraps_the_text_and_pages_it_under_its_bar() {
        assert_eq!(wrapped("the quick brown fox jumps\n\nend", 10), ["the quick ", "brown fox ", "jumps", "", "end"]);
        let lines: Vec<String> = (1..=30).map(|n| format!("line {n}")).collect();
        let out = page("The bulletin", &lines, 5, 60, 10);
        assert!(out.contains("\x1b[1;1H\x1b[7;36m\x1b[1m Preview: The bulletin\x1b[22m") && out.contains("lines 6–13 of 30"), "{out:?}");
        assert!(out.contains("\x1b[2;1H line 6") && out.contains("\x1b[9;1H line 13") && !out.contains("line 14"), "{out:?}");
        assert!(out.contains(&format!("\x1b[10;1H {DIM}↑ ↓ PgUp PgDn scroll")), "{out:?}");
        let out = page("Short", &lines[..3], 0, 40, 10);
        assert!(!out.contains("lines 1") && out.contains(&format!("{DIM}{BACK}")), "all of it in sight: no place, nothing to scroll: {out:?}");
    }

    #[test]
    fn a_live_pane_goes_beside_the_text_in_a_wide_window_and_under_it_in_a_narrow_one() {
        let live = |_: &Call, _: &str| Live::Words(String::new());
        let mut e = Editor::new(&Draft { hint: "Say hello", text: "first", live: Some(&live), ..Draft::new("Live") });
        assert_eq!(e.layout(120, 40), Layout { width: 58, height: 37, pane: Some(Pane { row: 2, col: 62, cols: 57, rows: 37, beside: true }) });
        let all = e.draw(120, 40, true);
        assert!(all.contains(&format!("\x1b[3;1H first{}{DIM}│{PLAIN}\x1b[4;1H", " ".repeat(54))), "the text up to the rule, the pane left alone: {all:?}");
        assert_eq!(e.layout(200, 40).pane.map(|p| (p.col, p.cols)), Some((120, 79)), "no wider than a card needs");
        assert_eq!(e.layout(80, 24), Layout { width: 78, height: 11, pane: Some(Pane { row: 13, col: 1, cols: 78, rows: 10, beside: false }) });
        let all = e.draw(80, 24, true);
        assert!(all.contains(&format!("\x1b[3;1H first{PLAIN}\x1b[K")) && all.contains("\x1b[24;1H \x1b[2mCtrl+S saves"), "the text over the pane, the keys under it: {all:?}");
        assert!(!all.contains("\x1b[14;1H") && all.contains("\x1b[13;1H "), "the pane's rows left to it: {all:?}");
        assert_eq!(e.layout(80, 12).pane, None, "no room for it");
        let plain = Editor::new(&Draft { text: "first", ..Draft::new("Not live") });
        assert_eq!(plain.layout(120, 40).pane, None, "none where the board draws none");
    }

    #[test]
    fn leaving_with_changes_asks_along_the_bottom() {
        let mut e = Editor::new(&Draft::new("Asked"));
        e.draw(80, 24, true);
        e.text.press(&Key::Char('x'), 78, 21);
        assert!(e.changed());
        e.asking = true;
        let out = e.draw(80, 24, false);
        assert!(out.contains(&format!("\x1b[24;1H \x1b[1;33m{LEAVING}")), "{out:?}");
    }
}
