//! A call: one caller on the board, from the moment their shell starts to
//! hang-up. The board's `call` runs on a thread of its own with it, reading
//! keys as they come and writing to the caller, while the server passes the
//! bytes to and from the SSH channel.
//!
//! A call knows whether the caller is in Flow before the board sees it:
//! from `TERM_PROGRAM=flow-terminal`, which Flow's `dial` sends; else from
//! `LC_TERMINAL=flow-terminal`, which crosses plain `ssh`; else by asking
//! the caller's terminal which it is (XTVERSION), with DA1 after it, which
//! every terminal answers, so the wait ends there. Keys typed while it
//! waits are kept for the board.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use crate::key::{self, Key, Read};

/// What Flow sets `TERM_PROGRAM` (and `LC_TERMINAL`) to.
pub const FLOW: &str = "flow-terminal";

/// How long a key cut off (an ESC alone) waits for the rest of it.
const REST_OF_A_KEY: Duration = Duration::from_millis(50);

/// The caller's window: cells, and pixels where their terminal says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
    /// The whole window's width and height in pixels; 0 where unknown.
    pub width: u32,
    pub height: u32,
}

impl Size {
    /// A cell's size in pixels, where the window's is known.
    pub fn cell(&self) -> Option<(u32, u32)> {
        (self.width > 0 && self.height > 0 && self.cols > 0 && self.rows > 0).then(|| (self.width / self.cols as u32, self.height / self.rows as u32))
    }
}

/// Who's calling, as far as the board can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// Flow, at this version where it said.
    Flow { version: Option<String> },
    /// Another terminal: `TERM`, and its name where it answered XTVERSION.
    Other { term: String, name: Option<String> },
}

/// What a call hears from the server.
#[derive(Debug)]
pub(crate) enum Event {
    /// Bytes from the caller: keys, and answers to what the board asked.
    Data(Vec<u8>),
    /// Their window's new size.
    Resize(Size),
}

/// What a call sends the server.
#[derive(Debug)]
pub(crate) enum Out {
    /// Bytes for the caller.
    Data(Vec<u8>),
    /// The board is done with the call: its exit status.
    End(u32),
}

/// One caller on the board.
pub struct Call {
    user: String,
    caller: Caller,
    size: Size,
    env: Vec<(String, String)>,
    events: Receiver<Event>,
    out: UnboundedSender<Out>,
    pending: VecDeque<u8>,
    hung_up: bool,
}

impl Call {
    pub(crate) fn new(user: String, term: String, size: Size, env: Vec<(String, String)>, events: Receiver<Event>, out: UnboundedSender<Out>) -> Self {
        let caller = Caller::Other { term, name: None };
        Call { user, caller, size, env, events, out, pending: VecDeque::new(), hung_up: false }
    }

    /// The name they logged in with (the board's guest login, for now).
    pub fn user(&self) -> &str {
        &self.user
    }

    /// Who's calling.
    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    /// Whether the caller is in Flow, so pictures, models, effects and
    /// sounds reach them.
    pub fn is_flow(&self) -> bool {
        matches!(self.caller, Caller::Flow { .. })
    }

    /// Their window's size, as it was last said.
    pub fn size(&self) -> Size {
        self.size
    }

    /// A variable their client sent (SSH's environment requests).
    pub fn env(&self, name: &str) -> Option<&str> {
        self.env.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// Whether they've hung up.
    pub fn hung_up(&self) -> bool {
        self.hung_up
    }

    /// Writes text to the caller, each newline as CR LF, since the call has
    /// no pty of its own to turn one into the other.
    pub fn say(&mut self, text: &str) -> io::Result<()> {
        self.write_all(text.replace("\r\n", "\n").replace('\n', "\r\n").as_bytes())
    }

    /// Writes an escape sequence as it is: one of Flow's SDK's pictures,
    /// models, effects or sounds, or the board's own.
    pub fn send(&mut self, escape: &str) -> io::Result<()> {
        self.write_all(escape.as_bytes())
    }

    /// The next key, waiting for it; `None` once they've hung up.
    pub fn key(&mut self) -> io::Result<Option<Key>> {
        loop {
            let bytes: Vec<u8> = self.pending.iter().copied().collect();
            match key::read(&bytes) {
                Read::Key(k, n) => {
                    self.pending.drain(..n);
                    return Ok(Some(k));
                }
                Read::Partial => match self.events.recv_timeout(REST_OF_A_KEY) {
                    Ok(event) => {
                        if let Some(k) = self.take(event) {
                            return Ok(Some(k));
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let (k, n) = key::cut_off(&bytes);
                        self.pending.drain(..n);
                        return Ok(Some(k));
                    }
                    Err(RecvTimeoutError::Disconnected) => return Ok(self.hang_up()),
                },
                Read::Empty => match self.events.recv() {
                    Ok(event) => {
                        if let Some(k) = self.take(event) {
                            return Ok(Some(k));
                        }
                    }
                    Err(_) => return Ok(self.hang_up()),
                },
            }
        }
    }

    /// Takes in what the server heard: bytes wait in `pending`, a resize is
    /// a key of its own.
    fn take(&mut self, event: Event) -> Option<Key> {
        match event {
            Event::Data(bytes) => {
                self.pending.extend(bytes);
                None
            }
            Event::Resize(size) => {
                self.size = size;
                Some(Key::Resize)
            }
        }
    }

    fn hang_up(&mut self) -> Option<Key> {
        self.hung_up = true;
        None
    }

    /// Finds out who's calling: Flow by its variables, else by asking the
    /// terminal, waiting at most `wait` for one that answers nothing.
    pub(crate) fn find_caller(&mut self, wait: Duration) {
        let said = |name: &str| self.env(name).is_some_and(|v| v == FLOW);
        if said("TERM_PROGRAM") || said("LC_TERMINAL") {
            let version = self.env("TERM_PROGRAM_VERSION").or(self.env("LC_TERMINAL_VERSION")).map(str::to_string);
            self.caller = Caller::Flow { version };
            return;
        }
        if self.send("\x1b[>0q\x1b[c").is_err() {
            return;
        }
        let deadline = Instant::now() + wait;
        let mut heard = Vec::new();
        loop {
            if let Some((name, used)) = answers(&heard) {
                self.pending.extend(strip(&heard, used));
                if let Some(version) = name.as_deref().and_then(|n| n.strip_prefix("Flow ")) {
                    self.caller = Caller::Flow { version: Some(version.to_string()) };
                } else if let Caller::Other { name: n, .. } = &mut self.caller {
                    *n = name;
                }
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.events.recv_timeout(left) {
                Ok(Event::Data(bytes)) => heard.extend(bytes),
                Ok(Event::Resize(size)) => self.size = size,
                Err(_) => {
                    // No answer in time: what came is the caller's keys.
                    self.pending.extend(heard);
                    return;
                }
            }
        }
    }
}

impl Write for Call {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.out.send(Out::Data(buf.to_vec())).is_err() {
            self.hung_up = true;
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "the caller hung up"));
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Finds the end of the terminal's answers in `heard`: DA1's
/// `ESC [ ? … c`. Gives XTVERSION's name where it came before it
/// (`ESC P >| name ESC \`), and the ranges the answers took, so the rest
/// can be kept as keys.
fn answers(heard: &[u8]) -> Option<Answers> {
    let da1 = find(heard, b"\x1b[?")?;
    let end = da1 + heard[da1..].iter().position(|&b| b == b'c')? + 1;
    let mut used = vec![(da1, end)];
    let mut name = None;
    if let Some(start) = find(&heard[..da1], b"\x1bP>|") {
        if let Some(close) = find(&heard[start..da1], b"\x1b\\") {
            name = Some(String::from_utf8_lossy(&heard[start + 4..start + close]).into_owned());
            used.push((start, start + close + 2));
        }
    }
    Some((name, used))
}

/// XTVERSION's name where the terminal gave one, and the byte ranges its
/// answers took.
type Answers = (Option<String>, Vec<(usize, usize)>);

/// `heard` without the ranges in `used`.
fn strip(heard: &[u8], used: Vec<(usize, usize)>) -> Vec<u8> {
    heard.iter().enumerate().filter(|(i, _)| !used.iter().any(|&(a, b)| (a..b).contains(i))).map(|(_, &b)| b).collect()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn call(env: &[(&str, &str)]) -> (Call, mpsc::Sender<Event>, tokio::sync::mpsc::UnboundedReceiver<Out>) {
        let (tx, rx) = mpsc::channel();
        let (out, sent) = tokio::sync::mpsc::unbounded_channel();
        let env = env.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        (Call::new("guest".into(), "xterm-256color".into(), Size { cols: 80, rows: 24, width: 800, height: 480 }, env, rx, out), tx, sent)
    }

    #[test]
    fn flow_is_known_by_its_variables_without_asking() {
        let (mut c, _tx, mut sent) = call(&[("TERM_PROGRAM", "flow-terminal"), ("TERM_PROGRAM_VERSION", "0.1.34")]);
        c.find_caller(Duration::from_millis(10));
        assert_eq!(c.caller(), &Caller::Flow { version: Some("0.1.34".into()) });
        assert!(sent.try_recv().is_err(), "nothing asked");
        let (mut c, _tx, _sent) = call(&[("LC_TERMINAL", "flow-terminal")]);
        c.find_caller(Duration::from_millis(10));
        assert!(c.is_flow());
    }

    #[test]
    fn otherwise_the_terminal_is_asked_and_flows_answer_counts() {
        let (mut c, tx, mut sent) = call(&[]);
        tx.send(Event::Data(b"q\x1bP>|Flow 0.1.35\x1b\\".to_vec())).unwrap();
        tx.send(Event::Data(b"\x1b[?62;22cz".to_vec())).unwrap();
        c.find_caller(Duration::from_secs(1));
        assert!(matches!(sent.try_recv(), Ok(Out::Data(d)) if d == b"\x1b[>0q\x1b[c"));
        assert_eq!(c.caller(), &Caller::Flow { version: Some("0.1.35".into()) });
        assert_eq!(c.key().unwrap(), Some(Key::Char('q')), "keys typed ahead are kept");
        assert_eq!(c.key().unwrap(), Some(Key::Char('z')));
    }

    #[test]
    fn another_terminal_is_named_where_it_says_and_one_that_answers_nothing_is_waited_for_no_longer() {
        let (mut c, tx, _sent) = call(&[]);
        tx.send(Event::Data(b"\x1bP>|kitty(0.36.4)\x1b\\\x1b[?62c".to_vec())).unwrap();
        c.find_caller(Duration::from_secs(1));
        assert_eq!(c.caller(), &Caller::Other { term: "xterm-256color".into(), name: Some("kitty(0.36.4)".into()) });
        let (mut c, tx, _sent) = call(&[]);
        tx.send(Event::Data(b"hi".to_vec())).unwrap();
        let start = Instant::now();
        c.find_caller(Duration::from_millis(100));
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!c.is_flow());
        assert_eq!(c.key().unwrap(), Some(Key::Char('h')), "what came is kept as keys");
    }

    #[test]
    fn keys_come_as_typed_a_resize_is_one_and_hang_up_ends_them() {
        let (mut c, tx, _sent) = call(&[]);
        tx.send(Event::Data(b"a\x1b".to_vec())).unwrap();
        tx.send(Event::Data(b"[A".to_vec())).unwrap();
        tx.send(Event::Resize(Size { cols: 100, rows: 30, width: 0, height: 0 })).unwrap();
        tx.send(Event::Data(b"\x1b".to_vec())).unwrap();
        assert_eq!(c.key().unwrap(), Some(Key::Char('a')));
        assert_eq!(c.key().unwrap(), Some(Key::Up), "an arrow split across two reads");
        assert_eq!(c.key().unwrap(), Some(Key::Resize));
        assert_eq!(c.size().cols, 100);
        assert_eq!(c.key().unwrap(), Some(Key::Esc), "ESC alone, nothing after it");
        drop(tx);
        assert_eq!(c.key().unwrap(), None);
        assert!(c.hung_up());
    }

    #[test]
    fn saying_turns_each_newline_into_cr_lf() {
        let (mut c, _tx, mut sent) = call(&[]);
        c.say("one\ntwo\r\nthree").unwrap();
        assert!(matches!(sent.try_recv(), Ok(Out::Data(d)) if d == b"one\r\ntwo\r\nthree"));
    }

    #[test]
    fn a_cell_is_the_window_over_its_cells() {
        assert_eq!(Size { cols: 80, rows: 24, width: 800, height: 480 }.cell(), Some((10, 20)));
        assert_eq!(Size { cols: 80, rows: 24, width: 0, height: 0 }.cell(), None);
    }
}
