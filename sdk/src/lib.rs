//! Flow's BBS SDK: boards that callers reach with Flow's `dial`, drawn with
//! what Flow adds (pictures, turning models, effects on the glass, sounds),
//! and in plain text and the sixteen colors for callers in other terminals.
//!
//! A board is a type with a [`Board::call`], which runs once for each
//! caller, on a thread of its own, given the [`Call`]: who's calling,
//! whether it's Flow, their window, their keys as they come, and a way to
//! write to them. [`serve`] puts the board behind an SSH server, so a
//! caller reaches it with `dial` or plain `ssh`:
//!
//! ```no_run
//! use flow_bbs::{serve, Call, Config, Key};
//!
//! fn main() -> std::io::Result<()> {
//!     serve(Config::default(), |call: &mut Call| {
//!         call.say(&format!("Welcome, {}. Press any key to hang up.\n", call.user()))?;
//!         if call.is_flow() {
//!             call.send(&flow_bbs::flow::fx::Effect::ripple((10, 0)).escape())?;
//!         }
//!         while let Some(key) = call.key()? {
//!             if key != Key::Resize {
//!                 break;
//!             }
//!         }
//!         Ok(())
//!     })
//! }
//! ```
//!
//! What Flow draws comes from Flow's SDK, re-exported as [`flow`]: its
//! pictures, models, effects and sounds are escape sequences as strings,
//! which [`Call::send`] writes down the call as they'd be written to a
//! terminal. Its own terminal handling (asking the terminal, raw keys) is
//! for a program at a terminal of its own, which a board isn't; [`Call`]
//! does that part for a call.

mod call;
mod key;
mod server;

use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub use call::{Call, Caller, Size, FLOW};
pub use flow_sdk as flow;
pub use key::Key;

/// A board: what each caller gets.
pub trait Board: Send + Sync + 'static {
    /// One call, from the start of the caller's shell to hang-up. It runs
    /// on a thread of its own; the call ends when it returns.
    fn call(&self, call: &mut Call) -> io::Result<()>;
}

impl<F> Board for F
where
    F: Fn(&mut Call) -> io::Result<()> + Send + Sync + 'static,
{
    fn call(&self, call: &mut Call) -> io::Result<()> {
        self(call)
    }
}

/// How a board is served.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where it listens: `0.0.0.0:2222` unless set, so the machine's own
    /// `sshd` keeps port 22.
    pub address: String,
    /// The board's host key, made there the first time (Ed25519). The
    /// phonebook pins it, so it's kept, not made again.
    pub host_key: PathBuf,
    /// The login callers use, with no password.
    pub guest: String,
    /// How long to wait for a caller's terminal to say which it is, where
    /// their variables haven't said it's Flow.
    pub ask: Duration,
    /// How long a call may sit with no traffic before it's hung up.
    pub idle: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            address: "0.0.0.0:2222".into(),
            host_key: PathBuf::from("host_key"),
            guest: "guest".into(),
            ask: Duration::from_secs(1),
            idle: Some(Duration::from_secs(60 * 60)),
        }
    }
}

/// Serves `board` at `config.address` until the process ends.
pub fn serve(config: Config, board: impl Board) -> io::Result<()> {
    let listener = TcpListener::bind(&config.address)?;
    serve_on(listener, config, board)
}

/// Serves `board` on a listener of the caller's own (a port picked by the
/// system, for tests), until the process ends; `config.address` is unused.
pub fn serve_on(listener: TcpListener, config: Config, board: impl Board) -> io::Result<()> {
    server::run(listener, config, Arc::new(board))
}
