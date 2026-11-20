//! `flow-doord`, the door server: a board's doors run apart from the board
//! (in a container of their own, as a user of their own, with no network),
//! which reaches them at a Unix socket (the SDK's `RemoteDoor`):
//!
//! ```text
//! flow-doord --socket /run/anie-doors/doors.sock --kept /var/lib/anie-doors [--most 32] \
//!     dungeon=/usr/local/bin/dungeon mines=/usr/local/bin/flow-mines
//! ```
//!
//! Each door is a name a board asks for it by and a program, with no
//! arguments. Folders kept between calls go under `--kept`; at most
//! `--most` doors run at once (32 unless set).

use std::collections::BTreeMap;
use std::process::ExitCode;

/// What the command line said: the socket, the kept folders, how many at
/// once, and the doors.
#[derive(Debug, PartialEq)]
struct Line {
    socket: String,
    kept: String,
    most: usize,
    doors: BTreeMap<String, String>,
}

const USAGE: &str = "usage: flow-doord --socket PATH --kept FOLDER [--most N] NAME=PROGRAM...";

fn line(args: impl IntoIterator<Item = String>) -> Result<Line, String> {
    let (mut socket, mut kept, mut most, mut doors) = (None, None, 32, BTreeMap::new());
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |what: &str| args.next().ok_or(format!("{what} needs a value"));
        match arg.as_str() {
            "--socket" => socket = Some(value("--socket")?),
            "--kept" => kept = Some(value("--kept")?),
            "--most" => most = value("--most")?.parse().ok().filter(|&n| n > 0).ok_or("--most is a number past 0")?,
            door => {
                let (name, program) = door.split_once('=').ok_or(format!("{door:?} isn't NAME=PROGRAM"))?;
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || program.is_empty() {
                    return Err(format!("{door:?} isn't NAME=PROGRAM, a name of letters, digits, - and _"));
                }
                if doors.insert(name.to_string(), program.to_string()).is_some() {
                    return Err(format!("the door {name} is named twice"));
                }
            }
        }
    }
    let socket = socket.ok_or("--socket must be given")?;
    let kept = kept.ok_or("--kept must be given")?;
    if doors.is_empty() {
        return Err("no doors to serve".into());
    }
    Ok(Line { socket, kept, most, doors })
}

fn main() -> ExitCode {
    let line = match line(std::env::args().skip(1)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("flow-doord: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    serve(line)
}

#[cfg(unix)]
fn serve(line: Line) -> ExitCode {
    use flow_bbs::Door;

    let doors = flow_bbs::Doors {
        socket: line.socket.into(),
        kept: line.kept.into(),
        doors: line.doors.into_iter().map(|(name, program)| (name, Door::new(program, Vec::<String>::new()))).collect(),
        most: line.most,
    };
    let names: Vec<&str> = doors.doors.keys().map(String::as_str).collect();
    let listener = match doors.listen() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("flow-doord: can't listen at {}: {e}", doors.socket.display());
            return ExitCode::FAILURE;
        }
    };
    eprintln!("flow-doord: {} at {}, {} at once", names.join(", "), doors.socket.display(), doors.most);
    match flow_bbs::serve_doors(doors, listener) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flow-doord: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn serve(_: Line) -> ExitCode {
    eprintln!("flow-doord: doors are served over a Unix socket, so it runs on Unix alone");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_line_names_the_socket_the_kept_folders_and_each_door_once() {
        let l = line(args("--socket /run/d.sock --kept /var/lib/d --most 8 dungeon=/usr/local/bin/dungeon mines=flow-mines")).unwrap();
        assert_eq!((l.socket.as_str(), l.kept.as_str(), l.most), ("/run/d.sock", "/var/lib/d", 8));
        assert_eq!(l.doors, BTreeMap::from([("dungeon".into(), "/usr/local/bin/dungeon".into()), ("mines".into(), "flow-mines".into())]));
        assert_eq!(line(args("--socket s --kept k neko=flow-cat")).unwrap().most, 32);
        for bad in ["--kept k neko=flow-cat", "--socket s neko=flow-cat", "--socket s --kept k", "--socket s --kept k neko", "--socket s --kept k ../x=y", "--socket s --kept k a=b a=c", "--socket s --kept k --most 0 a=b"] {
            assert!(line(args(bad)).is_err(), "{bad}");
        }
    }
}
