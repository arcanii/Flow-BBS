//! The keys a caller presses, read from the bytes their terminal sends down
//! the call: letters as they're typed (UTF-8), Enter, Backspace, Tab, Esc,
//! the arrows, and Ctrl with a letter. Anything else comes whole as
//! `Other`, so a board can still read what it knows: a CSI or SS3 to its
//! final byte, and a terminal's answer as a string (OSC, DCS, APC, PM, SOS)
//! to its end, so the letters in `ESC P >|Flow 0.1.35 ESC \` are never
//! taken for keys.

/// A key the caller pressed, or a change in their window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A letter, digit or sign, as typed.
    Char(char),
    /// Enter (or Return).
    Enter,
    /// Backspace.
    Backspace,
    /// Tab.
    Tab,
    /// Esc, pressed alone.
    Esc,
    /// The arrows.
    Up,
    Down,
    Left,
    Right,
    /// Ctrl with a letter, the letter lowercase: `Ctrl('c')`.
    Ctrl(char),
    /// The caller's window changed size: a board that draws to the size
    /// draws again.
    Resize,
    /// A sequence this doesn't name, whole.
    Other(Vec<u8>),
}

/// What reading a key from `bytes` found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// A key, and how many bytes it took.
    Key(Key, usize),
    /// The start of one: more bytes are needed (or, after a wait, it's Esc
    /// alone, or a letter cut off, taken as it is).
    Partial,
    /// Nothing there.
    Empty,
}

/// Reads the first key from `bytes`.
pub(crate) fn read(bytes: &[u8]) -> Read {
    let Some(&first) = bytes.first() else { return Read::Empty };
    let key = |k, n| Read::Key(k, n);
    match first {
        b'\r' => key(Key::Enter, if bytes.get(1) == Some(&b'\n') { 2 } else { 1 }),
        b'\n' => key(Key::Enter, 1),
        0x7f | 0x08 => key(Key::Backspace, 1),
        b'\t' => key(Key::Tab, 1),
        0x1b => escape(bytes),
        1..=0x1a => key(Key::Ctrl((b'a' + first - 1) as char), 1),
        0..=0x1f => key(Key::Other(vec![first]), 1),
        _ => letter(bytes),
    }
}

/// A key starting with ESC: an arrow's CSI or SS3, another sequence whole,
/// or Esc alone.
fn escape(bytes: &[u8]) -> Read {
    match bytes.get(1) {
        None => Read::Partial,
        Some(b'[') => {
            // Parameters and middle bytes, then one final byte from @ to ~.
            let Some(end) = bytes[2..].iter().position(|b| (0x40..=0x7e).contains(b)) else { return Read::Partial };
            let n = end + 3;
            let k = match (&bytes[2..n - 1], bytes[n - 1]) {
                (b"" | b"1", b'A') => Key::Up,
                (b"" | b"1", b'B') => Key::Down,
                (b"" | b"1", b'C') => Key::Right,
                (b"" | b"1", b'D') => Key::Left,
                _ => Key::Other(bytes[..n].to_vec()),
            };
            Read::Key(k, n)
        }
        Some(b'O') => match bytes.get(2) {
            None => Read::Partial,
            Some(b'A') => Read::Key(Key::Up, 3),
            Some(b'B') => Read::Key(Key::Down, 3),
            Some(b'C') => Read::Key(Key::Right, 3),
            Some(b'D') => Read::Key(Key::Left, 3),
            Some(_) => Read::Key(Key::Other(bytes[..3].to_vec()), 3),
        },
        Some(b']') => string(bytes, true),
        Some(b'P' | b'_' | b'^' | b'X') => string(bytes, false),
        Some(0x1b) => Read::Key(Key::Esc, 1),
        Some(_) => Read::Key(Key::Other(bytes[..2].to_vec()), 2),
    }
}

/// The longest string a call waits for whole; past it, what's come is taken
/// as it is.
const LONGEST_STRING: usize = 64 * 1024;

/// A string sequence (a terminal's answer, mostly): OSC, DCS, APC, PM or
/// SOS, whole to its ST (`ESC \`), or to BEL for an OSC (`bel`).
fn string(bytes: &[u8], bel: bool) -> Read {
    let body = &bytes[2..];
    let end = body.iter().enumerate().find_map(|(i, &b)| match b {
        0x07 if bel => Some(i + 1),
        0x1b if body.get(i + 1) == Some(&b'\\') => Some(i + 2),
        _ => None,
    });
    match end {
        Some(n) => Read::Key(Key::Other(bytes[..n + 2].to_vec()), n + 2),
        None if bytes.len() > LONGEST_STRING => Read::Key(Key::Other(bytes.to_vec()), bytes.len()),
        None => Read::Partial,
    }
}

/// Whether `bytes` start a string sequence, which may take longer to come
/// whole than a key.
pub(crate) fn in_string(bytes: &[u8]) -> bool {
    bytes.first() == Some(&0x1b) && matches!(bytes.get(1), Some(b']' | b'P' | b'_' | b'^' | b'X'))
}

/// A letter in UTF-8, or the byte alone where it isn't one.
fn letter(bytes: &[u8]) -> Read {
    let len = match bytes[0] {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Read::Key(Key::Other(vec![bytes[0]]), 1),
    };
    if bytes.len() < len {
        return Read::Partial;
    }
    match std::str::from_utf8(&bytes[..len]).ok().and_then(|s| s.chars().next()) {
        Some(c) => Read::Key(Key::Char(c), len),
        None => Read::Key(Key::Other(vec![bytes[0]]), 1),
    }
}

/// What's left of a partial read once no more bytes come: Esc alone, or the
/// bytes as they are.
pub(crate) fn cut_off(bytes: &[u8]) -> (Key, usize) {
    if bytes == [0x1b] {
        (Key::Esc, 1)
    } else {
        (Key::Other(bytes.to_vec()), bytes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(mut bytes: &[u8]) -> Vec<Key> {
        let mut out = Vec::new();
        while let Read::Key(k, n) = read(bytes) {
            out.push(k);
            bytes = &bytes[n..];
        }
        out
    }

    #[test]
    fn letters_and_the_plain_keys_are_read_as_typed() {
        assert_eq!(keys(b"aZ9 \r\x7f\t"), [Key::Char('a'), Key::Char('Z'), Key::Char('9'), Key::Char(' '), Key::Enter, Key::Backspace, Key::Tab]);
        assert_eq!(keys("é☕".as_bytes()), [Key::Char('é'), Key::Char('☕')]);
        assert_eq!(keys(b"\r\nx"), [Key::Enter, Key::Char('x')], "CR LF is one Enter");
        assert_eq!(keys(b"\x03\x11"), [Key::Ctrl('c'), Key::Ctrl('q')]);
    }

    #[test]
    fn arrows_come_as_csi_or_ss3_and_other_sequences_whole() {
        assert_eq!(keys(b"\x1b[A\x1b[B\x1bOC\x1b[1D"), [Key::Up, Key::Down, Key::Right, Key::Left]);
        assert_eq!(keys(b"\x1b[5~\x1b[1;5A"), [Key::Other(b"\x1b[5~".to_vec()), Key::Other(b"\x1b[1;5A".to_vec())]);
        assert_eq!(keys(b"\x1bx"), [Key::Other(b"\x1bx".to_vec())], "Alt with a letter");
    }

    #[test]
    fn a_terminals_answer_comes_whole_as_one_sequence_not_as_letters() {
        let xtversion = b"\x1bP>|Flow 0.1.35\x1b\\".to_vec();
        assert_eq!(keys(&[&xtversion[..], b"q"].concat()), [Key::Other(xtversion), Key::Char('q')], "a DCS, to its ST");
        assert_eq!(keys(b"\x1b]11;rgb:0000/0000/0000\x07a"), [Key::Other(b"\x1b]11;rgb:0000/0000/0000\x07".to_vec()), Key::Char('a')], "an OSC, to its BEL");
        assert_eq!(keys(b"\x1b]7732;e30=\x1b\\"), [Key::Other(b"\x1b]7732;e30=\x1b\\".to_vec())], "or to its ST");
        assert_eq!(keys(b"\x1b_Gi=31;OK\x1b\\"), [Key::Other(b"\x1b_Gi=31;OK\x1b\\".to_vec())], "kitty's graphics, an APC");
        assert_eq!(keys(b"\x1bP1$r\x1bm\x1b\\"), [Key::Other(b"\x1bP1$r\x1bm\x1b\\".to_vec())], "an ESC inside isn't its end");
    }

    #[test]
    fn a_string_cut_off_waits_for_the_rest() {
        for start in [&b"\x1bP>|Fl"[..], b"\x1b]11;rgb", b"\x1bP>|Flow\x1b"] {
            assert_eq!(read(start), Read::Partial, "{start:?}");
            assert!(in_string(start));
        }
        assert!(!in_string(b"\x1b[1;"));
        assert_eq!(cut_off(b"\x1bP>|Fl"), (Key::Other(b"\x1bP>|Fl".to_vec()), 6), "taken as it is once nothing more comes");
    }

    #[test]
    fn a_cut_off_key_waits_for_more_and_esc_alone_is_esc() {
        for start in [&b"\x1b"[..], b"\x1b[", b"\x1b[1;", b"\x1bO", "é".as_bytes().split_at(1).0] {
            assert_eq!(read(start), Read::Partial, "{start:?}");
        }
        assert_eq!(cut_off(b"\x1b"), (Key::Esc, 1));
        assert_eq!(keys(b"\x1b\x1b[A"), [Key::Esc, Key::Up]);
        assert_eq!(read(b""), Read::Empty);
    }
}
