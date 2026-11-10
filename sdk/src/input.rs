//! Lines a caller types: a prompt, their letters echoed as they come (or a
//! star for each, for a password), Backspace and Ctrl+U to take them back,
//! Enter to end and Esc to give up. Only printable letters are kept, so
//! nothing a caller types can carry an escape sequence onto another
//! caller's screen; `printable` does the same for text a board shows that
//! someone else wrote.

use std::io;

use unicode_width::UnicodeWidthChar;

use crate::call::Call;
use crate::key::Key;

impl Call {
    /// Says `prompt` and reads a line of at most `max` letters, echoed as
    /// typed. `None` when the caller gives up (Esc, Ctrl+C, Ctrl+D) or hangs
    /// up, which `hung_up` tells apart.
    pub fn ask(&mut self, prompt: &str, max: usize) -> io::Result<Option<String>> {
        self.line(prompt, max, false)
    }

    /// As `ask`, with a star for each letter: for passwords and codes.
    pub fn ask_secret(&mut self, prompt: &str, max: usize) -> io::Result<Option<String>> {
        self.line(prompt, max, true)
    }

    fn line(&mut self, prompt: &str, max: usize, secret: bool) -> io::Result<Option<String>> {
        self.say(prompt)?;
        let mut text = String::new();
        let shown = |c: char| if secret { '*' } else { c };
        loop {
            match self.key()? {
                None | Some(Key::Esc | Key::Ctrl('c' | 'd')) => {
                    if !self.hung_up() {
                        self.say("\n")?;
                    }
                    return Ok(None);
                }
                Some(Key::Enter) => {
                    self.say("\n")?;
                    return Ok(Some(text));
                }
                Some(Key::Backspace) => {
                    if let Some(c) = text.pop() {
                        self.send(&erase(shown(c)))?;
                    }
                }
                Some(Key::Ctrl('u')) => {
                    let all: String = text.chars().map(|c| erase(shown(c))).collect();
                    text.clear();
                    self.send(&all)?;
                }
                Some(Key::Char(c)) if keeps(c) && text.chars().count() < max => {
                    text.push(c);
                    let mut echo = [0; 4];
                    self.send(shown(c).encode_utf8(&mut echo))?;
                }
                Some(_) => {}
            }
        }
    }
}

/// Backs over one letter as wide as `c`, leaving blanks where it was.
fn erase(c: char) -> String {
    let w = c.width().unwrap_or(0).max(1);
    format!("{0}{1}{0}", "\x08".repeat(w), " ".repeat(w))
}

/// Whether a typed letter is kept: printable, and not one of the marks that
/// turn text around (which can make a line read as something it isn't).
pub(crate) fn keeps(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// `text` with nothing in it a terminal would act on: newlines stay, a tab
/// becomes spaces, and every other control (ESC with it, so no escape
/// sequence) and the marks that turn text around are left out. For text
/// one caller wrote that another will read.
pub fn printable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.replace("\r\n", "\n").chars() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            c if keeps(c) => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_text_keeps_lines_and_letters_and_drops_what_a_terminal_would_act_on() {
        assert_eq!(printable("one\r\ntwo\tthree"), "one\ntwo    three");
        assert_eq!(printable("\x1b[2J\x1b]0;owned\x07hi\u{9b}31m"), "[2J]0;ownedhi31m");
        assert_eq!(printable("abc\u{202e}fed"), "abcfed");
        assert_eq!(printable("浦和 ☕ é"), "浦和 ☕ é");
    }

    #[test]
    fn backing_over_a_wide_letter_takes_both_its_cells() {
        assert_eq!(erase('a'), "\x08 \x08");
        assert_eq!(erase('浦'), "\x08\x08  \x08\x08");
    }
}
