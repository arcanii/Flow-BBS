//! Menus: a screen above, the choices in a frame under it, and the
//! caller's pick. The arrows move a bar over the choices and Enter takes
//! the one it's on, or a choice's key takes it at once. A board's menus
//! lead to its screens, its other menus and its doors; what each choice
//! does is the board's. The frame is in the sixteen colors, so a caller's
//! theme recolors it, and its bar is the theme's own colors swapped.

use std::io;

use flow_sdk::fx::Sound;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::call::Call;
use crate::key::Key;
use crate::screen::Screen;

/// One of a menu's choices: the key that picks it, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: char,
    pub label: String,
}

impl Item {
    pub fn new(key: char, label: impl Into<String>) -> Item {
        Item { key, label: label.into() }
    }
}

/// What the menu says under its frame, so a caller knows the keys.
pub const MENU_KEYS: &str = "↑ ↓ move · Enter chooses · or press a key";

impl Call {
    /// Shows `screen` (if any) with the items in a frame under it, and
    /// waits for the caller to pick one, by the arrows and Enter or by its
    /// key, a letter's case aside: the item's index. `None` once the caller
    /// hangs up, or presses Ctrl+C or Ctrl+D, which mean the same here. A
    /// resize shows it all again, to the new size.
    pub fn menu(&mut self, screen: Option<&Screen>, values: &[(&str, &str)], items: &[Item]) -> io::Result<Option<usize>> {
        self.menu_titled(screen, values, "", items)
    }

    /// `menu`, its frame titled `title`.
    pub fn menu_titled(&mut self, screen: Option<&Screen>, values: &[(&str, &str)], title: &str, items: &[Item]) -> io::Result<Option<usize>> {
        let mut at = 0;
        loop {
            match screen {
                Some(s) => {
                    self.show_leaving(s, values, Frame::rows(items))?;
                }
                None => {
                    self.forget()?;
                    self.send("\x1b[2J\x1b[H")?;
                }
            }
            let cols = match self.size().cols {
                0 => 80,
                n => n,
            };
            let frame = Frame::new(title, items, cols);
            self.send(&frame.draw(at))?;
            loop {
                let pick = match self.key()? {
                    None | Some(Key::Ctrl('c' | 'd')) => return Ok(None),
                    Some(Key::Resize) => break,
                    Some(Key::Up | Key::Down) if items.is_empty() => None,
                    Some(Key::Up) => {
                        let to = (at + items.len() - 1) % items.len();
                        self.send(&frame.redraw(at, to))?;
                        at = to;
                        None
                    }
                    Some(Key::Down | Key::Tab) if !items.is_empty() => {
                        let to = (at + 1) % items.len();
                        self.send(&frame.redraw(at, to))?;
                        at = to;
                        None
                    }
                    Some(Key::Enter) if at < items.len() => Some(at),
                    Some(Key::Char(c)) => items.iter().position(|item| item.key.to_lowercase().eq(c.to_lowercase())),
                    Some(_) => None,
                };
                if let Some(i) = pick {
                    // The bar on what was picked, a click in Flow, and the
                    // cursor under the frame for what the board says next.
                    let mut out = frame.redraw(at, i);
                    if self.is_flow() {
                        out.push_str(&Sound::Click.escape(None, Some(0.35)));
                    }
                    out.push_str("\r\n");
                    self.send(&out)?;
                    return Ok(Some(i));
                }
            }
        }
    }

    /// Says `words` and waits for a key: `false` once the caller hangs up,
    /// or presses Ctrl+C or Ctrl+D.
    pub fn pause(&mut self, words: &str) -> io::Result<bool> {
        self.say(&format!("\n\x1b[2m{words}\x1b[0m"))?;
        loop {
            match self.key()? {
                None | Some(Key::Ctrl('c' | 'd')) => return Ok(false),
                Some(Key::Resize) => {}
                Some(_) => return Ok(true),
            }
        }
    }
}

/// A menu's frame, laid out for a window's width.
struct Frame {
    title: String,
    /// Each item's key, as it's shown, and its label, cut to fit.
    items: Vec<(String, String)>,
    /// The frame's width inside its sides, in cells.
    inner: usize,
}

/// The frame's color, and the bar's: cyan, and cyan swapped.
const LINE: &str = "\x1b[36m";
const BAR: &str = "\x1b[7;36m";
const KEY: &str = "\x1b[1;96m";
const PLAIN: &str = "\x1b[0m";

impl Frame {
    /// The rows a frame of `items` takes under a screen: a blank line, its
    /// top, the items, its bottom and the keys.
    fn rows(items: &[Item]) -> u16 {
        (items.len() as u16).saturating_add(4)
    }

    fn new(title: &str, items: &[Item], cols: u16) -> Frame {
        let keys: Vec<String> = items.iter().map(|i| i.key.to_uppercase().collect()).collect();
        let key_w = keys.iter().map(|k| k.width()).max().unwrap_or(1);
        let widest = items.iter().map(|i| i.label.width()).max().unwrap_or(0);
        // Two cells in from the window's edge, a side either side, and the
        // window's last cell left alone.
        let most = (cols as usize).saturating_sub(5).max(12);
        let inner = (widest + key_w + 7).max(title.width() + 6).max(30).min(most);
        let room = inner.saturating_sub(key_w + 6);
        let items = keys.into_iter().zip(items).map(|(k, i)| (k, cut(&i.label, room))).collect();
        Frame { title: cut(title, inner.saturating_sub(5)), items, inner }
    }

    /// The frame from the cursor's line, the bar on item `at`, the cursor
    /// left at the end of the keys under it.
    fn draw(&self, at: usize) -> String {
        let top = if self.title.is_empty() {
            format!("╭{}╮", "─".repeat(self.inner))
        } else {
            let fill = self.inner.saturating_sub(self.title.width() + 3);
            format!("╭─ {PLAIN}\x1b[1m{}{PLAIN}{LINE} {}╮", self.title, "─".repeat(fill))
        };
        let mut out = format!("\r\n  {LINE}{top}{PLAIN}\r\n");
        for i in 0..self.items.len() {
            out.push_str(&self.row(i, i == at));
            out.push_str("\r\n");
        }
        out.push_str(&format!("  {LINE}╰{}╯{PLAIN}\r\n", "─".repeat(self.inner)));
        out.push_str(&format!("  \x1b[2m{MENU_KEYS}{PLAIN}"));
        out
    }

    /// Item `i`'s row, with the bar on it or not.
    fn row(&self, i: usize, bar: bool) -> String {
        let (key, label) = &self.items[i];
        let used = 3 + key.width() + 2 + label.width();
        let pad = " ".repeat(self.inner.saturating_sub(used));
        if bar {
            format!("  {LINE}│{PLAIN}{BAR} ▸ \x1b[1m{key}\x1b[22m  {label}{pad}{PLAIN}{LINE}│{PLAIN}")
        } else {
            format!("  {LINE}│{PLAIN}   {KEY}{key}{PLAIN}  {label}{pad}{LINE}│{PLAIN}")
        }
    }

    /// The bar moved from item `from` to item `to`: their rows drawn again
    /// where they are, counted up from the keys' line, the cursor put back.
    fn redraw(&self, from: usize, to: usize) -> String {
        let n = self.items.len();
        let mut out = String::new();
        for (i, bar) in [(from, false), (to, true)] {
            if i < n && !(i == from && from == to && !bar) {
                out.push_str(&format!("\x1b7\x1b[{}A\r{}\x1b8", n + 1 - i, self.row(i, bar)));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                out.push(c);
            } else if chars.next() == Some('[') {
                // A CSI, to its final letter; ESC 7 and ESC 8 are gone already.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        }
        out
    }

    #[test]
    fn a_frame_is_as_wide_as_its_widest_label_and_every_row_as_wide_as_it() {
        let items = [Item::new('l', "Log in"), Item::new('a', "Apply for membership"), Item::new('q', "Hang up")];
        let frame = Frame::new("Welcome", &items, 80);
        let drawn = plain(&frame.draw(1));
        let lines: Vec<&str> = drawn.split("\r\n").collect();
        assert_eq!(lines[0], "", "a blank line above it");
        assert!(lines[1].starts_with("  ╭─ Welcome ─"), "{}", lines[1]);
        assert_eq!(frame.inner, 30, "at least thirty cells inside");
        assert_eq!(lines[2], format!("  │   L  Log in{}│", " ".repeat(30 - 12)));
        assert!(lines[3].starts_with("  │ ▸ A  Apply for membership"), "the bar on the second: {}", lines[3]);
        assert_eq!(lines[6], format!("  {MENU_KEYS}"));
        let widths: Vec<usize> = lines[1..6].iter().map(|l| l.width()).collect();
        assert!(widths.iter().all(|&w| w == widths[0]), "{widths:?}");
        assert_eq!(Frame::rows(&items), 7, "the rows it takes: a blank, its top, three, its bottom and the keys");
    }

    #[test]
    fn a_label_too_long_for_the_window_is_cut_with_an_ellipsis() {
        let items = [Item::new('g', "The gallery: a picture, a turning crystal, an effect, a sound")];
        let frame = Frame::new("", &items, 40);
        let row = plain(&frame.row(0, false));
        assert_eq!(row.width(), 2 + 1 + frame.inner + 1);
        assert!(row.contains("  The gallery: a picture, a t… │"), "{row}");
        assert!(row.width() < 40);
    }

    #[test]
    fn moving_the_bar_draws_the_two_rows_again_counted_up_from_the_keys() {
        let items = [Item::new('a', "One"), Item::new('b', "Two"), Item::new('c', "Three")];
        let frame = Frame::new("", &items, 80);
        let moved = frame.redraw(0, 2);
        assert!(moved.starts_with("\x1b7\x1b[4A\r"), "the first item is four up: {moved:?}");
        assert!(moved.contains("\x1b8\x1b7\x1b[2A\r"), "the third two up: {moved:?}");
        assert!(plain(&moved).contains("▸ C  Three"));
    }
}
