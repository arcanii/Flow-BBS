//! How the board draws what isn't a screen file: a heading, text wrapped to
//! the caller's window, a long text a screenful at a time, questions, and a
//! line editor for messages. What a member wrote is always shown through
//! `printable`, so it can't carry an escape onto another member's screen.

use std::io;

use flow_bbs::{printable, Call};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const CYAN: &str = "\x1b[1;36m";
pub const YELLOW: &str = "\x1b[1;33m";
pub const RED: &str = "\x1b[1;31m";
pub const GREEN: &str = "\x1b[1;32m";
pub const PLAIN: &str = "\x1b[0m";

/// The caller's window, or 80 × 24 where it's unsaid.
pub fn window(call: &Call) -> (usize, usize) {
    let s = call.size();
    (if s.cols == 0 { 80 } else { s.cols as usize }, if s.rows == 0 { 24 } else { s.rows as usize })
}

/// Clears the screen and writes `title` across its top.
pub fn heading(call: &mut Call, title: &str) -> io::Result<()> {
    let (cols, _) = window(call);
    call.clear()?;
    let rule = "─".repeat(cols.saturating_sub(2).min(76));
    call.say(&format!(" {CYAN}{}{PLAIN}\n {DIM}{rule}{PLAIN}\n", printable(title)))
}

/// `text` cut to lines of at most `width` cells, at spaces where it can be;
/// a word too long for a line is broken where it has to be.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut lines = Vec::new();
    for para in printable(text).split('\n') {
        let mut line = String::new();
        let mut w = 0;
        for word in para.split(' ') {
            let ww = word.width();
            if w > 0 && w + 1 + ww > width {
                lines.push(std::mem::take(&mut line));
                w = 0;
            }
            if ww > width {
                for c in word.chars() {
                    let cw = c.width().unwrap_or(0);
                    if w + cw > width {
                        lines.push(std::mem::take(&mut line));
                        w = 0;
                    }
                    line.push(c);
                    w += cw;
                }
                continue;
            }
            if w > 0 {
                line.push(' ');
                w += 1;
            }
            line.push_str(word);
            w += ww;
        }
        lines.push(line);
    }
    lines
}

/// `text` cut to `width` cells, with an ellipsis where it was cut.
pub fn cut(text: &str, width: usize) -> String {
    let text = printable(text).replace('\n', " ");
    if text.width() <= width {
        return text;
    }
    let mut out = String::new();
    let mut w = 0;
    for c in text.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// `text` padded with spaces to `width` cells (or cut to them).
pub fn pad(text: &str, width: usize) -> String {
    let text = cut(text, width);
    let w = text.width();
    format!("{text}{}", " ".repeat(width.saturating_sub(w)))
}

/// Shows `lines` under `title`, a screenful at a time. `false` once the
/// caller hangs up; `true` when they've read them, or stopped.
pub fn page(call: &mut Call, title: &str, lines: &[String]) -> io::Result<bool> {
    let (_, rows) = window(call);
    let per = rows.saturating_sub(4).max(5);
    let mut at = 0;
    loop {
        heading(call, title)?;
        for line in lines.iter().skip(at).take(per) {
            call.say(&format!(" {line}\n"))?;
        }
        at += per;
        if at >= lines.len() {
            return Ok(true);
        }
        let shown = at.min(lines.len());
        match call.ask(&format!("{DIM}-- {shown} of {} lines: Enter for more, Q to stop --{PLAIN} ", lines.len()), 1)? {
            None if call.hung_up() => return Ok(false),
            None => return Ok(true),
            Some(s) if s.eq_ignore_ascii_case("q") => return Ok(true),
            Some(_) => {}
        }
    }
}

/// Asks a yes or no question; no unless they say yes. `None` once they've
/// hung up.
pub fn yes(call: &mut Call, question: &str) -> io::Result<Option<bool>> {
    match call.ask(&format!("{question} (y/N) "), 3)? {
        None if call.hung_up() => Ok(None),
        None => Ok(Some(false)),
        Some(a) => Ok(Some(matches!(a.trim().to_ascii_lowercase().as_str(), "y" | "yes"))),
    }
}

/// What they typed at a prompt, trimmed: `None` if they gave up or hung up.
pub fn answer(call: &mut Call, prompt: &str, max: usize) -> io::Result<Option<String>> {
    Ok(call.ask(prompt, max)?.map(|s| s.trim().to_string()))
}

/// Says `words` in the board's way of saying something went wrong.
pub fn oops(call: &mut Call, words: &str) -> io::Result<()> {
    call.say(&format!(" {RED}{words}{PLAIN}\n"))
}

/// Says `words` in the board's way of saying it's done.
pub fn done(call: &mut Call, words: &str) -> io::Result<()> {
    call.say(&format!(" {GREEN}{words}{PLAIN}\n"))
}

/// The most a message may be.
pub const MESSAGE_LINES: usize = 200;
pub const MESSAGE_BYTES: usize = 16 * 1024;

/// What a line typed into the editor asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Save,
    Abort,
    List,
    TakeBack,
    Clear,
    Help,
    Text(String),
}

impl Line {
    pub fn read(typed: &str) -> Line {
        match typed.trim().to_ascii_lowercase().as_str() {
            "/s" => Line::Save,
            "/a" => Line::Abort,
            "/l" => Line::List,
            "/d" => Line::TakeBack,
            "/c" => Line::Clear,
            "/?" | "/h" => Line::Help,
            _ => Line::Text(typed.trim_end().to_string()),
        }
    }
}

const EDITOR_HELP: &str = "Type your message a line at a time. On a line of its own: /s saves it, /a gives it up, /l lists it, /d takes back the last line, /c starts again.";

/// A message typed a line at a time, starting from `start`: `None` if it's
/// given up (or they hang up).
pub fn editor(call: &mut Call, start: &str) -> io::Result<Option<String>> {
    let (cols, _) = window(call);
    let mut lines: Vec<String> = if start.is_empty() { Vec::new() } else { printable(start).lines().map(str::to_string).collect() };
    for l in wrap(EDITOR_HELP, cols.saturating_sub(2)) {
        call.say(&format!(" {DIM}{l}{PLAIN}\n"))?;
    }
    if !lines.is_empty() {
        list(call, &lines)?;
    }
    loop {
        let Some(typed) = call.ask(&format!("{DIM}{:>3}:{PLAIN} ", lines.len() + 1), cols.saturating_sub(6).clamp(20, 200))? else {
            if call.hung_up() {
                return Ok(None);
            }
            match yes(call, " Give up this message?")? {
                Some(true) | None => return Ok(None),
                Some(false) => continue,
            }
        };
        match Line::read(&typed) {
            Line::Save => {
                let text = lines.join("\n").trim().to_string();
                if text.is_empty() {
                    oops(call, "There's nothing to save yet. /a gives it up.")?;
                } else {
                    return Ok(Some(text));
                }
            }
            Line::Abort => return Ok(None),
            Line::List => list(call, &lines)?,
            Line::TakeBack => {
                if lines.pop().is_some() {
                    call.say(&format!(" {DIM}Took back line {}.{PLAIN}\n", lines.len() + 1))?;
                }
            }
            Line::Clear => {
                lines.clear();
                call.say(&format!(" {DIM}Starting again.{PLAIN}\n"))?;
            }
            Line::Help => call.say(&format!(" {DIM}{EDITOR_HELP}{PLAIN}\n"))?,
            Line::Text(t) => {
                let size: usize = lines.iter().map(|l| l.len() + 1).sum::<usize>() + t.len();
                if lines.len() >= MESSAGE_LINES || size > MESSAGE_BYTES {
                    oops(call, "That's as long as a message can be: /s saves it.")?;
                } else {
                    lines.push(t);
                }
            }
        }
    }
}

fn list(call: &mut Call, lines: &[String]) -> io::Result<()> {
    for (i, l) in lines.iter().enumerate() {
        call.say(&format!("{DIM}{:>3}:{PLAIN} {l}\n", i + 1))?;
    }
    Ok(())
}

/// A QR code of `text` in half blocks, two rows of modules to a line of
/// text, dark on light with a margin of light round it, as a phone's camera
/// reads it in any terminal.
pub fn qr(text: &str) -> Vec<String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let Ok(code) = QrCode::encode_text(text, QrCodeEcc::Low) else { return Vec::new() };
    let margin = 2;
    let size = code.size();
    let dark = |x: i32, y: i32| (0..size).contains(&x) && (0..size).contains(&y) && code.get_module(x, y);
    let mut lines = Vec::new();
    let mut y = -margin;
    while y < size + margin {
        let mut line = String::from("\x1b[30;107m");
        for x in -margin..size + margin {
            line.push(match (dark(x, y), dark(x, y + 1)) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        line.push_str(PLAIN);
        lines.push(line);
        y += 2;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wraps_at_spaces_to_the_width_and_breaks_a_word_too_long() {
        assert_eq!(wrap("the quick brown fox jumps", 10), ["the quick", "brown fox", "jumps"]);
        assert_eq!(wrap("one\n\ntwo", 10), ["one", "", "two"]);
        assert_eq!(wrap("abcdefghijklmnop", 10), ["abcdefghij", "klmnop"]);
        assert_eq!(wrap("浦和レッズの試合", 10), ["浦和レッズ", "の試合"], "wide letters take two cells");
        assert_eq!(wrap("\x1b[31mred", 10), ["[31mred"], "no escapes");
    }

    #[test]
    fn a_cut_line_ends_in_an_ellipsis_and_a_padded_one_fills_its_width() {
        assert_eq!(cut("hello world", 20), "hello world");
        assert_eq!(cut("hello world", 8), "hello w…");
        assert_eq!(pad("hi", 5), "hi   ");
        assert_eq!(pad("浦和", 5), "浦和 ");
    }

    #[test]
    fn the_editor_knows_its_commands_on_a_line_of_their_own() {
        assert_eq!(Line::read("/s"), Line::Save);
        assert_eq!(Line::read(" /A "), Line::Abort);
        assert_eq!(Line::read("/l"), Line::List);
        assert_eq!(Line::read("/d"), Line::TakeBack);
        assert_eq!(Line::read("/s is how you save"), Line::Text("/s is how you save".into()));
        assert_eq!(Line::read("  indented  "), Line::Text("  indented".into()));
    }

    #[test]
    fn a_qr_code_is_square_in_modules_with_its_margin() {
        let lines = qr("otpauth://totp/Anie%20Solutions%20BBS:bryan?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Anie%20Solutions%20BBS");
        let width = lines[0].trim_start_matches("\x1b[30;107m").trim_end_matches(PLAIN).chars().count();
        assert!(width >= 25 + 4, "{width}");
        assert_eq!(lines.len(), width.div_ceil(2), "two rows of modules a line");
        assert!(lines.iter().all(|l| l.starts_with("\x1b[30;107m") && l.ends_with(PLAIN)));
    }
}
