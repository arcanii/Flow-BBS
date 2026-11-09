//! flow-cat: Neko, the cat that chases the pointer, as a door for Flow's
//! BBSes (and a toy in a Flow tab of its own). In Flow it's Neko's own
//! pictures, its frames sent once and shown by placeholder cells moved where
//! it runs; it sits, washes, scratches, yawns and sleeps when it's caught up,
//! claws at the edge the pointer's past, and Flow's liquid ripples when it
//! pounces. In another terminal it's a cat in
//! text. The pointer is the mouse; the arrows move a mark for it to chase
//! where there's no mouse. `q` or Esc leaves.
//!
//! It's Neko, the desktop cat of 1989: its frames are Kenji Gotoh's, free to
//! use, modify and redistribute (`sprites/NOTICE.md`), and what it does is
//! Neko's, in `cat.rs`.

mod cat;
mod sprites;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use flow_sdk::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use flow_sdk::crossterm::{cursor, execute, terminal};
use flow_sdk::fx::{Effect, Sound};
use flow_sdk::picture::{self, Image};
use flow_sdk::{Query, Terminal};

use crate::cat::{Cat, Moment};
use crate::sprites::{Frame, SIZE};

/// A tick: eight a second, as the old cat ran.
const TICK: Duration = Duration::from_millis(125);

/// How many rows tall the cat is, in Flow; in text it's three.
const ROWS: u16 = 3;

/// The first picture id, clear of the low ones programs pick.
const BASE: u32 = picture::Ids::FIRST + 0x2C_0000;

const STATUS: &str = "Move the pointer (or the arrows): the cat chases it.  q leaves.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("flow-cat {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            println!("flow-cat: a cat that chases the pointer.\n\n{STATUS}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("flow-cat: {other}: it takes no arguments\nusage: flow-cat");
            return ExitCode::from(2);
        }
        None => {}
    }
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        eprintln!("flow-cat: the cat needs a terminal");
        return ExitCode::FAILURE;
    }
    match play() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flow-cat: {e}");
            ExitCode::FAILURE
        }
    }
}

/// How the cat is drawn here.
enum Look {
    /// Pictures shown by placeholder cells, `w` × `h` of them.
    Pictures { w: u16, h: u16 },
    /// Pictures down Flow's pipe on Windows, each row its own picture.
    Bare { w: u16, h: u16 },
    /// Text.
    Text,
}

impl Look {
    fn size(&self) -> (u16, u16) {
        match self {
            Look::Pictures { w, h } | Look::Bare { w, h } => (*w, *h),
            Look::Text => (8, 3),
        }
    }
}

fn play() -> io::Result<()> {
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(terminal::ClearType::All))?;
    let mut term = Query::new().pictures().cell().area().wait(Duration::from_millis(800)).ask();
    let done = chase(&mut term, &mut out);
    let _ = term.send(Frame::all().into_iter().flat_map(|f| (0..ROWS).map(move |r| picture::delete(id(f, r)))).collect::<String>().as_bytes());
    let _ = execute!(out, DisableMouseCapture, terminal::LeaveAlternateScreen, cursor::Show);
    let _ = terminal::disable_raw_mode();
    done
}

/// A cell's size in pixels, from what the terminal said, its window's
/// pixels, or a common size.
fn cell(term: &Terminal) -> (f32, f32) {
    if let Some((w, h)) = term.caps.cell.filter(|(w, h)| *w > 0 && *h > 0) {
        return (w as f32, h as f32);
    }
    match terminal::window_size() {
        Ok(s) if s.width > 0 && s.columns > 0 && s.rows > 0 => (s.width as f32 / s.columns as f32, s.height as f32 / s.rows as f32),
        _ => (9.0, 18.0),
    }
}

/// Sends the frames' pictures, each once, and says how the cat is drawn.
fn send_frames(term: &mut Terminal, cell: (f32, f32)) -> io::Result<Look> {
    if !term.caps.pictures {
        return Ok(Look::Text);
    }
    let h = ROWS;
    let w = ((h as f32 * cell.1) / cell.0).round().max(1.0) as u16;
    let scale = ((h as f32 * cell.1) / SIZE as f32).ceil().max(1.0) as usize;
    let side = (SIZE * scale) as u32;
    let mut sent = String::new();
    for frame in Frame::all() {
        let image = Image::rgba(side, side, sprites::rgba(frame, scale));
        if term.caps.bare {
            for r in 0..h {
                let strip = image.strip(r, h).unwrap_or_else(|| image.clone());
                sent.push_str(&picture::upload(id(frame, r), &strip, 0, w, 1));
            }
        } else {
            sent.push_str(&picture::upload(id(frame, 0), &image, 1, w, h));
        }
    }
    term.send(sent.as_bytes())?;
    Ok(if term.caps.bare { Look::Bare { w, h } } else { Look::Pictures { w, h } })
}

/// A frame's picture id; on Windows each of its rows has its own.
fn id(frame: Frame, row: u16) -> u32 {
    BASE + frame.index() as u32 * ROWS as u32 + row as u32
}

/// The cat drawn at (`col`, `row`).
fn draw(look: &Look, frame: Frame, col: u16, row: u16) -> String {
    match look {
        Look::Pictures { w, h } => picture::placeholders(id(frame, 0), 1, (col, row), *w, *h),
        Look::Bare { w, h } => (0..*h).map(|r| format!("\x1b[{};{}H{}", row + r + 1, col + 1, picture::bare_row(id(frame, r), *w))).collect(),
        Look::Text => sprites::text(frame).iter().enumerate().map(|(r, line)| format!("\x1b[{};{}H\x1b[1m{line}\x1b[0m", row + r as u16 + 1, col + 1)).collect(),
    }
}

/// Spaces over a `w` × `h` place at (`col`, `row`).
fn erase(col: u16, row: u16, w: u16, h: u16) -> String {
    (0..h).map(|r| format!("\x1b[{};{}H{}", row + r + 1, col + 1, " ".repeat(w as usize))).collect()
}

fn chase(term: &mut Terminal, out: &mut io::Stdout) -> io::Result<()> {
    let cell = cell(term);
    let aspect = cell.1 / cell.0;
    let look = send_frames(term, cell)?;
    execute!(out, EnableMouseCapture)?;
    let flow = flow_sdk::in_flow() || term.caps.flow;
    let (mut cols, mut rows) = terminal::size()?;
    let (w, h) = look.size();
    let mut cat = Cat::new(cols.saturating_sub(w) as f32 / 2.0, rows.saturating_sub(h + 1) as f32 / 2.0, w as f32, h as f32);
    let mut target: Option<(f32, f32)> = None;
    // The mark the arrows move, where they're used.
    let mut mark: Option<(u16, u16)> = None;
    let mut shown: Option<(u16, u16)> = None;
    let mut fresh = true;
    let mut next = Instant::now();
    loop {
        if fresh {
            let status: String = STATUS.chars().take(cols as usize).collect();
            write!(out, "\x1b[2J\x1b[{};1H\x1b[2m{status}\x1b[0m", rows)?;
            shown = None;
            fresh = false;
        }
        let wait = next.saturating_duration_since(Instant::now());
        if event::poll(wait)? {
            match event::read()? {
                Event::Mouse(m) if matches!(m.kind, MouseEventKind::Moved | MouseEventKind::Drag(_) | MouseEventKind::Down(_)) => {
                    target = Some((m.column as f32, m.row as f32));
                    if let Some((c, r)) = mark.take() {
                        write!(out, "{}", erase(c, r, 1, 1))?;
                    }
                }
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    let step = |by: (i32, i32)| {
                        let (c, r) = mark.unwrap_or(((cat.x + cat.w / 2.0) as u16, (cat.y + cat.h / 2.0) as u16));
                        let c = (c as i32 + by.0).clamp(0, cols.saturating_sub(1) as i32) as u16;
                        let r = (r as i32 + by.1).clamp(0, rows.saturating_sub(2) as i32) as u16;
                        (c, r)
                    };
                    let moved = match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char('c' | 'd') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                        KeyCode::Left => Some(step((-4, 0))),
                        KeyCode::Right => Some(step((4, 0))),
                        KeyCode::Up => Some(step((0, -2))),
                        KeyCode::Down => Some(step((0, 2))),
                        _ => None,
                    };
                    if let Some((c, r)) = moved {
                        if let Some((oc, or)) = mark {
                            write!(out, "{}", erase(oc, or, 1, 1))?;
                        }
                        mark = Some((c, r));
                        target = Some((c as f32, r as f32));
                    }
                }
                Event::Resize(c, r) => {
                    (cols, rows) = (c, r);
                    fresh = true;
                }
                _ => {}
            }
            continue;
        }
        next += TICK;
        if next < Instant::now() {
            next = Instant::now() + TICK;
        }
        let moment = cat.tick(target, cols as f32, rows.saturating_sub(1) as f32, aspect);
        let (col, row) = (cat.x.round().max(0.0) as u16, cat.y.round().max(0.0) as u16);
        let mut screen = String::new();
        if let Some((c, r)) = shown {
            screen.push_str(&erase(c, r, w, h));
        }
        screen.push_str(&draw(&look, cat.frame(), col, row));
        if let Some((c, r)) = mark {
            screen.push_str(&format!("\x1b[{};{}H\x1b[1;36m+\x1b[0m", r + 1, c + 1));
        }
        out.write_all(screen.as_bytes())?;
        out.flush()?;
        shown = Some((col, row));
        if flow {
            let at = target.map(|(c, r)| (c as u16, r as u16)).unwrap_or((col + w / 2, row + h / 2));
            let effect = match moment {
                Some(Moment::Caught) => Effect::ripple(at).strength(0.4).escape(),
                Some(Moment::Woke) => Sound::Boing.escape(Some((col + w / 2, row)), Some(0.3)),
                None => String::new(),
            };
            term.send_effects(effect.as_bytes())?;
        }
    }
}
