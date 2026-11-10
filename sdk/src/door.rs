//! Doors: programs a board hands the call to, as BBSes always have, run
//! on a pty of the call's own so they can be written in any language and
//! know nothing of the board. The caller's keys go to the door as they
//! come, and what it writes goes back to the caller as it is; a resize
//! resizes its pty, pixels and all. A door for a caller in Flow has
//! `TERM_PROGRAM=flow-terminal` in its environment, so Flow's games draw
//! their pictures and effects, and when they ask the terminal its version
//! or a cell's size, the questions reach the caller's Flow and the answers
//! come back down. A door gets none of the server's own environment but
//! what a program needs to start (`PATH`, `HOME`, the language and time
//! zone), so a board's settings and secrets never reach one; it gets the
//! caller's `TERM` and Flow's variables, and what the board gives it
//! (`Door::env`). When the door ends, the call goes back to the board.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::call::{Call, Caller, Event, Out, Size, FLOW};

/// A program for a board to hand the call to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Door {
    /// The program, found on the server's PATH where it's a bare name.
    pub program: OsString,
    pub args: Vec<OsString>,
    /// The folder it runs in; the board's own unless set.
    pub dir: Option<PathBuf>,
    /// Variables of its own, beside the caller's `TERM` and Flow's.
    pub env: Vec<(String, String)>,
}

impl Door {
    /// A door that runs `program` with `args`.
    pub fn new<S: Into<OsString>>(program: S, args: impl IntoIterator<Item = S>) -> Door {
        Door { program: program.into(), args: args.into_iter().map(Into::into).collect(), dir: None, env: Vec::new() }
    }
}

/// What a door keeps of the server's environment: where programs are, its
/// home, its language and time zone, and on Windows what a program needs
/// to start at all.
const KEPT: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LANGUAGE", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "TZ", "TMPDIR",
    "SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT", "TEMP", "TMP", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "ProgramData",
];

/// How often the call looks to see whether the door has ended.
const LOOK: Duration = Duration::from_millis(50);

impl Call {
    /// Runs `door` for the caller until it ends, and gives its exit code;
    /// `None` if the caller hung up first, which ends the door.
    pub fn door(&mut self, door: &Door) -> io::Result<Option<u32>> {
        let pty = native_pty_system().openpty(pty_size(self.size)).map_err(io::Error::other)?;
        let mut command = CommandBuilder::new(&door.program);
        command.args(&door.args);
        if let Some(dir) = &door.dir {
            command.cwd(dir);
        }
        // A door is any program, facing whoever calls: it gets none of the
        // server's own variables (a board's settings and secrets) but what
        // a program needs to start.
        command.env_clear();
        for name in KEPT {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command.env("TERM", self.term());
        if let Caller::Flow { version } = &self.caller {
            command.env("TERM_PROGRAM", FLOW);
            if let Some(v) = version {
                command.env("TERM_PROGRAM_VERSION", v);
            }
        }
        for (name, value) in &door.env {
            command.env(name, value);
        }
        let mut child = pty.slave.spawn_command(command).map_err(|e| io::Error::other(format!("can't run {}: {e}", door.program.to_string_lossy())))?;
        drop(pty.slave);
        let mut keys = pty.master.take_writer().map_err(io::Error::other)?;
        let mut output = pty.master.try_clone_reader().map_err(io::Error::other)?;
        let out = self.out.clone();
        let copying = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = output.read(&mut buf) {
                if n == 0 || out.send(Out::Data(buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        });
        // Keys typed before the door opened are the door's.
        if !self.pending.is_empty() {
            let typed: Vec<u8> = self.pending.drain(..).collect();
            keys.write_all(&typed)?;
        }
        let ended = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status.exit_code());
            }
            match self.events.recv_timeout(LOOK) {
                Ok(Event::Data(bytes)) => {
                    // A door that has ended can't take keys; it's seen
                    // ending on the next look.
                    let _ = keys.write_all(&bytes);
                }
                Ok(Event::Resize(size)) => {
                    self.size = size;
                    let _ = pty.master.resize(pty_size(size));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.hung_up = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        drop(keys);
        drop(pty.master);
        let _ = copying.join();
        Ok(ended)
    }
}

/// The pty's size from the caller's window, pixels where it said them.
fn pty_size(size: Size) -> PtySize {
    let px = |n: u32| n.min(u16::MAX as u32) as u16;
    PtySize { rows: size.rows.max(1), cols: size.cols.max(1), pixel_width: px(size.width), pixel_height: px(size.height) }
}
