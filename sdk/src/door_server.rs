//! The door server: doors run for boards elsewhere, as `flow-doord` serves
//! them, at a Unix socket (remote.rs says what goes over it). Each
//! connection is one door for one caller: its folder made (kept between
//! calls under `kept`, `<kept>/<door>/<name>`, by the name the board gives,
//! or one in the temporary folder for the call alone, removed when the door
//! ends; either the server's user's alone), the files the board gives put
//! there, its program run in it on a pty of its own as a door on the
//! board's is (none of the server's variables but what a program needs to
//! start, the caller's `TERM` and Flow's), with `HOME` and `XDG_DATA_HOME`
//! the folder, so a door keeps what it keeps there. A door that's asked
//! for and isn't the server's is refused, and so is one past `most` at
//! once.
//!
//! What the board sends is checked, though only a board that can reach the
//! socket sends anything: a folder's name is letters, digits, `-` and `_`;
//! a file's path is in the folder (no `..`, no leading `/` or `.`); a file
//! given back is a plain file, not a link, and text. The socket is anyone's
//! to connect to (mode 666), so its folder decides who reaches it.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::native_pty_system;
use serde_json::{json, Map, Value};

use crate::call::{Caller, Size};
use crate::door::{command, pty_size, Door};
use crate::remote::{object, read_frame, seconds, size_from, write_frame, EXIT, HELLO, KEYS, OUTPUT, READY, REFUSED, RESIZE};

/// The doors a server runs, and where.
#[derive(Debug, Clone)]
pub struct Doors {
    /// Its socket, made when it starts (a stale one there is replaced).
    pub socket: PathBuf,
    /// Where the folders kept between calls are.
    pub kept: PathBuf,
    /// Its doors, by the names boards ask for them by. A door's own folder
    /// (`Door::dir`) is set for each call; the rest is as it's given.
    pub doors: BTreeMap<String, Door>,
    /// How many doors run at once, at most.
    pub most: usize,
}

/// The most files a hello puts, looks at or takes, and the longest name or
/// `TERM` it may give.
const FILES: usize = 16;
const NAME: usize = 64;

/// How long an ended door's output has to finish before its end is said.
const DRAIN: Duration = Duration::from_secs(2);

impl Doors {
    /// Listens at the socket: a stale socket there is removed first, and
    /// the new one is anyone's to connect to.
    pub fn listen(&self) -> io::Result<UnixListener> {
        if let Ok(there) = fs::symlink_metadata(&self.socket) {
            if !there.file_type().is_socket() {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} is there and isn't a socket", self.socket.display())));
            }
            fs::remove_file(&self.socket)?;
        }
        let listener = UnixListener::bind(&self.socket)?;
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(0o666))?;
        Ok(listener)
    }
}

/// Serves `doors` on `listener` (from `Doors::listen`) until it fails,
/// each connection on threads of its own.
pub fn serve_doors(doors: Doors, listener: UnixListener) -> io::Result<()> {
    let doors = Arc::new(doors);
    let running = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("flow-doord: a connection failed: {e}");
                continue;
            }
        };
        let (doors, running) = (doors.clone(), running.clone());
        std::thread::spawn(move || {
            if let Err(e) = connection(&doors, &running, stream) {
                eprintln!("flow-doord: {e}");
            }
        });
    }
    Ok(())
}

/// A count of doors running, given back when it's dropped.
struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// What a hello asks for, checked.
#[derive(Debug, PartialEq)]
struct Asked {
    door: String,
    keep: Option<String>,
    term: String,
    caller: Caller,
    size: Size,
    put: Vec<(String, String)>,
    look: Vec<String>,
    take: Vec<String>,
}

/// A name of a folder kept: letters, digits, `-` and `_`.
fn a_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= NAME && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A file's path in a door's folder: a few parts of letters, digits, `.`,
/// `-` and `_`, none of them starting with `.`.
fn a_path(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    s.len() <= 4 * NAME && parts.len() <= 4 && parts.iter().all(|p| !p.is_empty() && !p.starts_with('.') && p.len() <= NAME && p.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)))
}

/// What a hello says, or why it's refused.
fn asked(payload: &[u8]) -> Result<Asked, String> {
    let hello = object(payload).map_err(|_| "a hello that isn't JSON".to_string())?;
    let text = |k: &str| hello.get(k).and_then(Value::as_str);
    let door = text("door").filter(|d| a_name(d)).ok_or("no door")?.to_string();
    let keep = match hello.get("keep") {
        None | Some(Value::Null) => None,
        Some(Value::String(k)) if a_name(k) => Some(k.clone()),
        _ => return Err("a folder's name that isn't one".into()),
    };
    let printable = |s: &str| !s.is_empty() && s.len() <= NAME && s.chars().all(|c| c.is_ascii_graphic());
    let term = text("term").filter(|t| printable(t)).ok_or("no TERM")?.to_string();
    let caller = if hello.get("flow").and_then(Value::as_bool) == Some(true) {
        Caller::Flow { version: text("flow_version").filter(|v| printable(v)).map(str::to_string) }
    } else {
        Caller::Other { term: term.clone(), name: None }
    };
    let n = |i: usize| hello.get("size").and_then(|s| s.get(i)).and_then(Value::as_u64).unwrap_or(0);
    let size = Size { cols: n(0).min(u16::MAX as u64) as u16, rows: n(1).min(u16::MAX as u64) as u16, width: n(2).min(u32::MAX as u64) as u32, height: n(3).min(u32::MAX as u64) as u32 };
    let paths = |k: &str| -> Result<Vec<String>, String> {
        let Some(list) = hello.get(k) else { return Ok(Vec::new()) };
        let list = list.as_array().ok_or(format!("{k} isn't a list"))?;
        if list.len() > FILES {
            return Err(format!("too many files to {k}"));
        }
        list.iter().map(|p| p.as_str().filter(|p| a_path(p)).map(str::to_string).ok_or(format!("a path to {k} that isn't in the folder"))).collect()
    };
    let (look, take) = (paths("look")?, paths("take")?);
    let put = match hello.get("put") {
        None => Vec::new(),
        Some(Value::Object(files)) if files.len() <= FILES => files
            .iter()
            .map(|(p, t)| match t.as_str() {
                Some(t) if a_path(p) => Ok((p.clone(), t.to_string())),
                _ => Err("a file to put that isn't text in the folder".to_string()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("files to put that aren't".into()),
    };
    Ok(Asked { door, keep, term, caller, size, put, look, take })
}

/// Makes a folder that's the server's user's alone, and the folders above
/// it where `above`.
fn own_folder(folder: &Path, above: bool) -> io::Result<()> {
    fs::DirBuilder::new().recursive(above).mode(0o700).create(folder)
}

/// A file in a folder that's a plain file and not a link, or nothing.
fn plain_file(folder: &Path, path: &str) -> Option<(PathBuf, fs::Metadata)> {
    let file = folder.join(path);
    let there = fs::symlink_metadata(&file).ok()?;
    there.file_type().is_file().then_some((file, there))
}

/// Puts a text file in the folder, the folders on its way made first, a
/// link there replaced rather than followed.
fn put(folder: &Path, path: &str, text: &str) -> io::Result<()> {
    let file = folder.join(path);
    if let Some(above) = file.parent() {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(above)?;
    }
    if fs::symlink_metadata(&file).is_ok_and(|m| !m.file_type().is_file()) {
        fs::remove_file(&file)?;
    }
    fs::write(&file, text)
}

/// The files given back: those there, plain and text, none past a frame's
/// room between them.
fn taken(folder: &Path, take: &[String]) -> Map<String, Value> {
    let mut room = crate::remote::LONGEST / 2;
    let mut files = Map::new();
    for path in take {
        let Some((file, there)) = plain_file(folder, path) else { continue };
        if there.len() as usize > room {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else { continue };
        room -= text.len();
        files.insert(path.clone(), Value::from(text));
    }
    files
}

fn refuse(stream: &mut UnixStream, why: &str) -> io::Result<()> {
    write_frame(stream, REFUSED, json!({ "why": why }).to_string().as_bytes())
}

/// One connection: one door for one caller, from hello to end.
fn connection(doors: &Doors, running: &Arc<AtomicUsize>, mut stream: UnixStream) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let Some((HELLO, payload)) = read_frame(&mut stream)? else { return refuse(&mut stream, "no hello") };
    stream.set_read_timeout(None)?;
    let asked = match asked(&payload) {
        Ok(a) => a,
        Err(why) => return refuse(&mut stream, &why),
    };
    let Some(door) = doors.doors.get(&asked.door) else { return refuse(&mut stream, "no such door") };
    if running.fetch_add(1, Ordering::SeqCst) >= doors.most {
        running.fetch_sub(1, Ordering::SeqCst);
        return refuse(&mut stream, "as many doors as it takes are running");
    }
    let count = Running(running.clone());

    // Its folder: kept, or the call's alone.
    let (folder, scratch) = match &asked.keep {
        Some(name) => (doors.kept.join(&asked.door).join(name), false),
        None => (std::env::temp_dir().join(format!("flow-doors-{}-{:016x}", asked.door, rand::random::<u64>())), true),
    };
    let made = own_folder(&folder, !scratch).and_then(|_| asked.put.iter().try_for_each(|(p, t)| put(&folder, p, t)));
    if let Err(e) = made {
        if scratch {
            let _ = fs::remove_dir_all(&folder);
        }
        refuse(&mut stream, "its folder can't be made")?;
        return Err(io::Error::other(format!("{}: its folder: {e}", asked.door)));
    }
    let looked: Map<String, Value> = asked.look.iter().filter_map(|p| Some((p.clone(), Value::from(seconds(plain_file(&folder, p)?.1.modified().ok()?))))).collect();

    let ran = run(door, &asked, &folder, scratch, &mut stream, looked, count);
    if scratch {
        let _ = fs::remove_dir_all(&folder);
    }
    ran
}

/// Runs the door in its folder and passes the call through until it ends
/// or the board goes, then says how it ended: the call's own folder gone
/// first, so the board never finds it still there.
fn run(door: &Door, asked: &Asked, folder: &Path, scratch: bool, stream: &mut UnixStream, looked: Map<String, Value>, count: Running) -> io::Result<()> {
    let mut door = door.clone();
    door.dir = Some(folder.to_path_buf());
    let home = folder.to_string_lossy().into_owned();
    door.env.extend([("HOME".to_string(), home.clone()), ("XDG_DATA_HOME".to_string(), home)]);
    let pty = native_pty_system().openpty(pty_size(asked.size)).map_err(io::Error::other)?;
    let mut child = match pty.slave.spawn_command(command(&door, &asked.term, &asked.caller)) {
        Ok(child) => child,
        Err(e) => {
            refuse(stream, "it won't start")?;
            return Err(io::Error::other(format!("{}: can't run {}: {e}", asked.door, door.program.to_string_lossy())));
        }
    };
    drop(pty.slave);
    let mut killer = child.clone_killer();
    let mut keys = pty.master.take_writer().map_err(io::Error::other)?;
    let mut output = pty.master.try_clone_reader().map_err(io::Error::other)?;
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    write_frame(&mut *writer.lock().unwrap(), READY, json!({ "looked": looked }).to_string().as_bytes())?;

    // What the door writes goes to the board as it comes.
    let (drained_tx, drained_rx) = mpsc::channel::<()>();
    let out = writer.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = output.read(&mut buf) {
            if n == 0 || write_frame(&mut *out.lock().unwrap(), OUTPUT, &buf[..n]).is_err() {
                break;
            }
        }
        let _ = drained_tx.send(());
    });

    // When it ends: the rest of its output, then its end and what it
    // leaves, then the connection closed, which ends the board's keys too.
    let (folder, take, name) = (folder.to_path_buf(), asked.take.clone(), asked.door.clone());
    let end = writer.clone();
    let waiting = std::thread::spawn(move || {
        let status = child.wait();
        let _ = drained_rx.recv_timeout(DRAIN);
        let code = status.as_ref().ok().map(|s| s.exit_code());
        let said = json!({ "code": code, "taken": taken(&folder, &take) });
        if scratch {
            let _ = fs::remove_dir_all(&folder);
        }
        let mut end = end.lock().unwrap();
        if let Err(e) = write_frame(&mut *end, EXIT, said.to_string().as_bytes()) {
            if e.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("flow-doord: {name}: its end: {e}");
            }
        }
        let _ = end.shutdown(Shutdown::Both);
        drop(count);
    });

    // The board's keys and resizes, until it's done or the door is.
    loop {
        match read_frame(stream) {
            Ok(Some((KEYS, bytes))) => {
                let _ = keys.write_all(&bytes);
            }
            Ok(Some((RESIZE, b))) => {
                if let Some(size) = size_from(&b) {
                    let _ = pty.master.resize(pty_size(size));
                }
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    let _ = killer.kill();
    drop(keys);
    let _ = waiting.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folders_name_and_a_files_path_stay_in_the_folder() {
        assert!(a_name("12") && a_name("guest-room_2"));
        for bad in ["", "../12", "a/b", "a b", "ä", &"x".repeat(65)] {
            assert!(!a_name(bad), "{bad:?}");
        }
        assert!(a_path("dsave.dat") && a_path("flow-mines/best.json"));
        for bad in ["", "/etc/passwd", "../dsave.dat", "a/../b", ".profile", "a//b", "a/b/c/d/e", "a\\b"] {
            assert!(!a_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_hello_is_taken_only_as_it_should_be() {
        let ok = br#"{"door": "dungeon", "keep": "12", "term": "xterm-256color", "flow": true, "flow_version": "0.9", "size": [80, 24, 800, 480], "look": ["dsave.dat"]}"#;
        let a = asked(ok).unwrap();
        assert_eq!((a.door.as_str(), a.keep.as_deref(), a.term.as_str()), ("dungeon", Some("12"), "xterm-256color"));
        assert_eq!(a.caller, Caller::Flow { version: Some("0.9".into()) });
        assert_eq!(a.size, Size { cols: 80, rows: 24, width: 800, height: 480 });
        assert_eq!(a.look, ["dsave.dat"]);
        for bad in [
            &br#"{"door": "dungeon", "keep": "../bbs", "term": "xterm"}"#[..],
            br#"{"door": "dungeon", "term": "xterm", "take": ["/var/lib/anie-bbs/host_key"]}"#,
            br#"{"door": "dungeon", "term": "xterm", "put": {"../x": "y"}}"#,
            br#"{"door": "dungeon", "term": "x term"}"#,
            br#"{"term": "xterm"}"#,
            b"not json",
        ] {
            assert!(asked(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn only_plain_text_files_are_given_back() {
        let dir = std::env::temp_dir().join(format!("flow-doors-taken-{}", std::process::id()));
        own_folder(&dir, true).unwrap();
        put(&dir, "flow-mines/best.json", "{\"Small\": 9.5}").unwrap();
        fs::write(dir.join("bytes"), [0xff, 0xfe]).unwrap();
        std::os::unix::fs::symlink("/etc/hostname", dir.join("link")).unwrap();
        let t = taken(&dir, &["flow-mines/best.json".into(), "bytes".into(), "link".into(), "none".into()]);
        assert_eq!(t.keys().collect::<Vec<_>>(), ["flow-mines/best.json"]);
        put(&dir, "link", "a file now").unwrap();
        assert_eq!(fs::read_to_string(dir.join("link")).unwrap(), "a file now", "the link replaced, not followed");
        assert_eq!(fs::metadata(dir.join("flow-mines")).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = fs::remove_dir_all(&dir);
    }
}
