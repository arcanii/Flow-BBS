//! Doors served elsewhere: a door a door server runs (`serve_doors`, as
//! `flow-doord` serves them), reached at a Unix socket, so a board can keep
//! a program its callers type at, and the files it keeps, out of its own
//! reach (a container of their own, as a user of their own, with no
//! network). The board says which door, the caller's terminal and window,
//! and the folder it runs in: one it keeps between calls by a name the
//! board gives (a member's), or one for the call alone, gone when the door
//! ends. It can put small text files there first, ask when files there
//! were last changed, and have files given back when the door ends; then
//! the caller's keys go to the door and what it writes comes back, as a
//! door on the board's own pty does.
//!
//! On the socket, each side writes frames: a byte saying what it is, its
//! length (four bytes, most significant first) and that many bytes. The
//! board says hello (JSON), then keys and resizes; the server answers
//! ready (JSON: the files' times) or refused (JSON: why), then output, and
//! last how the door ended (JSON: its exit code and the files given back).
//! The board shutting its side ends the door.

// Elsewhere than Unix there's no socket to reach a server at, and no server.
#![cfg_attr(not(unix), allow(dead_code))]

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::call::{Call, Caller, Size};

/// What a frame is.
pub(crate) const HELLO: u8 = b'H';
pub(crate) const KEYS: u8 = b'K';
pub(crate) const RESIZE: u8 = b'Z';
pub(crate) const READY: u8 = b'R';
pub(crate) const REFUSED: u8 = b'N';
pub(crate) const OUTPUT: u8 = b'O';
pub(crate) const EXIT: u8 = b'E';

/// The longest frame either side takes: a hello with its files, or a
/// door's end with what it gives back.
pub(crate) const LONGEST: usize = 1 << 20;

/// How long the board waits for the server to say the door's ready, and,
/// after a hang-up, for what the ended door gives back.
const ANSWER: Duration = Duration::from_secs(10);
const LEFT: Duration = Duration::from_secs(5);

/// Writes one frame whole, so frames from two threads never interleave
/// under one lock.
pub(crate) fn write_frame(w: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(kind);
    frame.extend((payload.len() as u32).to_be_bytes());
    frame.extend(payload);
    w.write_all(&frame)
}

/// Reads one frame; `None` at the end of the stream.
pub(crate) fn read_frame(r: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match r.read_exact(&mut head) {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        done => done?,
    }
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > LONGEST {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a frame of {len} bytes")));
    }
    let mut payload = vec![0; len];
    r.read_exact(&mut payload)?;
    Ok(Some((head[0], payload)))
}

/// A window as a resize frame says it: cells, then pixels.
pub(crate) fn size_bytes(size: Size) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0..2].copy_from_slice(&size.cols.to_be_bytes());
    b[2..4].copy_from_slice(&size.rows.to_be_bytes());
    b[4..8].copy_from_slice(&size.width.to_be_bytes());
    b[8..12].copy_from_slice(&size.height.to_be_bytes());
    b
}

pub(crate) fn size_from(b: &[u8]) -> Option<Size> {
    let b: &[u8; 12] = b.try_into().ok()?;
    Some(Size {
        cols: u16::from_be_bytes([b[0], b[1]]),
        rows: u16::from_be_bytes([b[2], b[3]]),
        width: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
        height: u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
    })
}

/// A door a door server runs, and what the board asks of its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDoor {
    /// The server's socket.
    pub socket: PathBuf,
    /// The door, by the name the server knows it by.
    pub door: String,
    /// The folder kept between calls, by a name of the board's (letters,
    /// digits, `-` and `_`): a member's own. Unset, the door gets a folder
    /// of the call's alone, gone when it ends.
    pub keep: Option<String>,
    /// Text files put in the folder before the door starts, by their paths
    /// in it (as `flow-mines/best.json`).
    pub put: Vec<(String, String)>,
    /// Files in the folder whose last change the server says as the door
    /// opens (`OpenDoor::modified`).
    pub look: Vec<String>,
    /// Text files in the folder given back when the door ends
    /// (`Ended::taken`).
    pub take: Vec<String>,
}

impl RemoteDoor {
    /// The door `door` of the server at `socket`, in a folder of the call's.
    pub fn new(socket: impl Into<PathBuf>, door: impl Into<String>) -> RemoteDoor {
        RemoteDoor { socket: socket.into(), door: door.into(), keep: None, put: Vec::new(), look: Vec::new(), take: Vec::new() }
    }
}

/// A door the server has made ready, its folder made and its program
/// started, for `Call::run_door` to hand the call to.
pub struct OpenDoor {
    #[cfg(unix)]
    stream: std::os::unix::net::UnixStream,
    looked: BTreeMap<String, SystemTime>,
}

impl OpenDoor {
    /// When `file` (one the board asked to look at) was last changed, if
    /// it's there.
    pub fn modified(&self, file: &str) -> Option<SystemTime> {
        self.looked.get(file).copied()
    }
}

/// How a door served elsewhere ended.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ended {
    /// Its exit code; `None` where the caller hung up first, which ended it.
    pub code: Option<u32>,
    /// The files asked for that were there when it ended, as text.
    pub taken: BTreeMap<String, String>,
}

/// The hello for `door`, from a caller at `term`, as `caller`, in a window
/// of `size`.
fn hello(door: &RemoteDoor, term: &str, caller: &Caller, size: Size) -> Vec<u8> {
    let (flow, version) = match caller {
        Caller::Flow { version } => (true, version.clone()),
        Caller::Other { .. } => (false, None),
    };
    let put: serde_json::Map<String, Value> = door.put.iter().map(|(name, text)| (name.clone(), Value::from(text.as_str()))).collect();
    json!({
        "door": door.door,
        "keep": door.keep,
        "term": term,
        "flow": flow,
        "flow_version": version,
        "size": [size.cols, size.rows, size.width, size.height],
        "put": put,
        "look": door.look,
        "take": door.take,
    })
    .to_string()
    .into_bytes()
}

/// A JSON frame's object; an error where it isn't one.
pub(crate) fn object(payload: &[u8]) -> io::Result<serde_json::Map<String, Value>> {
    match serde_json::from_slice(payload) {
        Ok(Value::Object(o)) => Ok(o),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "a frame that isn't a JSON object")),
    }
}

/// A time as seconds since 1970, as a ready frame gives it.
pub(crate) fn seconds(at: SystemTime) -> f64 {
    at.duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn time_from(secs: f64) -> Option<SystemTime> {
    (secs.is_finite() && secs >= 0.0).then(|| UNIX_EPOCH + Duration::from_secs_f64(secs.min(1e12)))
}

/// The files' times in a ready frame.
fn looked(payload: &[u8]) -> io::Result<BTreeMap<String, SystemTime>> {
    let ready = object(payload)?;
    let Some(Value::Object(times)) = ready.get("looked") else { return Ok(BTreeMap::new()) };
    Ok(times.iter().filter_map(|(name, t)| Some((name.clone(), time_from(t.as_f64()?)?))).collect())
}

/// How the door ended, from its last frame.
fn ended(payload: &[u8]) -> io::Result<Ended> {
    let end = object(payload)?;
    let code = end.get("code").and_then(Value::as_u64).map(|c| c.min(u32::MAX as u64) as u32);
    let taken = match end.get("taken") {
        Some(Value::Object(files)) => files.iter().filter_map(|(name, text)| Some((name.clone(), text.as_str()?.to_string()))).collect(),
        _ => BTreeMap::new(),
    };
    Ok(Ended { code, taken })
}

impl Call {
    /// Asks the door server for `door`: its folder made and the files put
    /// there, its program started. An error where there's no server at the
    /// socket, or it refuses (a door it hasn't, or as many running as it
    /// takes).
    pub fn open_door(&mut self, door: &RemoteDoor) -> io::Result<OpenDoor> {
        #[cfg(unix)]
        {
            let mut stream = std::os::unix::net::UnixStream::connect(&door.socket)?;
            write_frame(&mut stream, HELLO, &hello(door, self.term(), &self.caller, self.size))?;
            stream.set_read_timeout(Some(ANSWER))?;
            let looked = match read_frame(&mut stream)? {
                Some((READY, payload)) => looked(&payload)?,
                Some((REFUSED, payload)) => {
                    let why = object(&payload).ok().and_then(|o| o.get("why").and_then(Value::as_str).map(str::to_string));
                    return Err(io::Error::other(format!("the door server refused {}: {}", door.door, why.unwrap_or_default())));
                }
                _ => return Err(io::Error::other("the door server didn't answer")),
            };
            stream.set_read_timeout(None)?;
            Ok(OpenDoor { stream, looked })
        }
        #[cfg(not(unix))]
        {
            let _ = door;
            Err(io::Error::new(io::ErrorKind::Unsupported, "a door served elsewhere is reached by a Unix socket"))
        }
    }

    /// Hands the call to a door `open_door` opened until it ends, and says
    /// how it ended. If the caller hangs up first, the door is ended, and
    /// what it leaves is still given back.
    pub fn run_door(&mut self, open: OpenDoor) -> io::Result<Ended> {
        #[cfg(unix)]
        {
            use std::net::Shutdown;
            use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};

            use crate::call::{Event, Out};
            use crate::door::LOOK;

            let mut writer = open.stream;
            let mut reader = writer.try_clone()?;
            let (end_tx, end_rx) = mpsc::channel();
            let out = self.out.clone();
            let reading = std::thread::spawn(move || {
                let end = loop {
                    match read_frame(&mut reader) {
                        Ok(Some((OUTPUT, bytes))) => {
                            // A call that's gone takes nothing more, but the
                            // door's end is still wanted.
                            let _ = out.send(Out::Data(bytes));
                        }
                        Ok(Some((EXIT, payload))) => break ended(&payload),
                        Ok(Some(_)) => {}
                        Ok(None) => break Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the door server went away")),
                        Err(e) => break Err(e),
                    }
                };
                let _ = end_tx.send(end);
            });
            // Keys typed before the door opened are the door's.
            if !self.pending.is_empty() {
                let typed: Vec<u8> = self.pending.drain(..).collect();
                let _ = write_frame(&mut writer, KEYS, &typed);
            }
            let end = loop {
                match end_rx.try_recv() {
                    Ok(end) => break end,
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => break Err(io::Error::other("the door's end was lost")),
                }
                match self.events.recv_timeout(LOOK) {
                    // A door that has ended can't take keys; its end comes
                    // next.
                    Ok(Event::Data(bytes)) => {
                        let _ = write_frame(&mut writer, KEYS, &bytes);
                    }
                    Ok(Event::Resize(size)) => {
                        self.size = size;
                        let _ = write_frame(&mut writer, RESIZE, &size_bytes(size));
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        self.hung_up = true;
                        let _ = writer.shutdown(Shutdown::Write);
                        let end = end_rx.recv_timeout(LEFT).unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "the door didn't end")));
                        break end.map(|e| Ended { code: None, ..e });
                    }
                }
            };
            let _ = writer.shutdown(Shutdown::Both);
            let _ = reading.join();
            end
        }
        #[cfg(not(unix))]
        {
            let _ = open;
            Err(io::Error::new(io::ErrorKind::Unsupported, "a door served elsewhere is reached by a Unix socket"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_comes_back_as_it_went_and_one_too_long_is_refused() {
        let mut wire = Vec::new();
        write_frame(&mut wire, KEYS, b"look").unwrap();
        write_frame(&mut wire, RESIZE, &size_bytes(Size { cols: 80, rows: 24, width: 800, height: 480 })).unwrap();
        let mut r = &wire[..];
        assert_eq!(read_frame(&mut r).unwrap(), Some((KEYS, b"look".to_vec())));
        let (kind, b) = read_frame(&mut r).unwrap().unwrap();
        assert_eq!((kind, size_from(&b)), (RESIZE, Some(Size { cols: 80, rows: 24, width: 800, height: 480 })));
        assert_eq!(read_frame(&mut r).unwrap(), None, "the end of the stream");
        let mut long = vec![OUTPUT];
        long.extend(((LONGEST + 1) as u32).to_be_bytes());
        assert!(read_frame(&mut &long[..]).is_err());
    }

    #[test]
    fn a_hello_says_the_door_the_caller_and_the_folder_and_an_end_gives_back_only_text() {
        let mut door = RemoteDoor::new("/run/doors.sock", "mines");
        door.keep = Some("12".into());
        door.put.push(("flow-mines/best.json".into(), "{}".into()));
        door.take.push("flow-mines/best.json".into());
        let said = object(&hello(&door, "xterm-256color", &Caller::Flow { version: Some("0.9".into()) }, Size { cols: 80, rows: 24, width: 0, height: 0 })).unwrap();
        assert_eq!(said["door"], "mines");
        assert_eq!((said["keep"].as_str(), said["flow"].as_bool(), said["flow_version"].as_str()), (Some("12"), Some(true), Some("0.9")));
        assert_eq!(said["put"]["flow-mines/best.json"], "{}");
        assert_eq!(said["size"], json!([80, 24, 0, 0]));

        let end = ended(br#"{"code": 3, "taken": {"a": "text", "b": 7}}"#).unwrap();
        assert_eq!(end, Ended { code: Some(3), taken: BTreeMap::from([("a".into(), "text".into())]) });
        let at = looked(br#"{"looked": {"dsave.dat": 1763600000.5, "bad": -1}}"#).unwrap();
        assert_eq!(at.keys().collect::<Vec<_>>(), ["dsave.dat"]);
    }
}
