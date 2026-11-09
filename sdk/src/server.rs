//! The SSH server a board runs behind, on russh's server half. Each caller
//! logs in as the board's guest (no password; sign-ups come later), asks
//! for a pty (its `TERM` and window, in cells and pixels), sends what
//! environment it likes (Flow's `dial` sends `TERM_PROGRAM`), and asks for
//! a shell: that starts the board's `call` on a thread of its own. Their
//! keys go to the call as they come; what the call writes goes back down
//! the channel; when the call returns, the channel closes with its exit
//! status. A command (`ssh board ls`) is refused: a board is no shell.
//! Past the board's limits (connections open at once, and from one
//! address), a new connection is turned away before it logs in.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use tokio::sync::mpsc::unbounded_channel;

use crate::call::{Call, Event, Out, Size};
use crate::{Board, Config};

/// Runs the board on `listener` until the process ends.
pub(crate) fn run(listener: std::net::TcpListener, config: Config, board: Arc<dyn Board>) -> io::Result<()> {
    let key = host_key(&config.host_key)?;
    let ssh = russh::server::Config {
        keys: vec![key],
        auth_rejection_time: Duration::from_secs(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        inactivity_timeout: config.idle,
        ..Default::default()
    };
    listener.set_nonblocking(true)?;
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(async move {
        let socket = tokio::net::TcpListener::from_std(listener)?;
        let mut server = Listener { board, config: Arc::new(config), open: Arc::default() };
        server.run_on_socket(Arc::new(ssh), &socket).await
    })
}

/// The board's host key, read from `path`, or made there (Ed25519, only the
/// owner may read it) the first time, so a board keeps one key for the
/// phonebook to pin.
pub(crate) fn host_key(path: &Path) -> io::Result<PrivateKey> {
    if path.exists() {
        return russh::keys::load_secret_key(path, None).map_err(|e| io::Error::other(format!("can't read the host key {}: {e}", path.display())));
    }
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).map_err(io::Error::other)?;
    let text = key.to_openssh(LineEnding::LF).map_err(io::Error::other)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    io::Write::write_all(&mut options.open(path)?, text.as_bytes())?;
    Ok(key)
}

struct Listener {
    board: Arc<dyn Board>,
    config: Arc<Config>,
    open: Arc<Mutex<Open>>,
}

/// The connections open now: how many, and from where.
#[derive(Default)]
struct Open {
    total: usize,
    by_address: HashMap<IpAddr, usize>,
}

/// A connection's place among those open, given back when it ends.
struct Slot {
    open: Arc<Mutex<Open>>,
    ip: Option<IpAddr>,
}

impl Slot {
    /// A place for a connection from `address`, if the limits leave one.
    fn take(open: &Arc<Mutex<Open>>, address: Option<SocketAddr>, config: &Config) -> Option<Slot> {
        let ip = address.map(|a| a.ip());
        let mut o = open.lock().unwrap_or_else(|e| e.into_inner());
        let here = ip.map_or(0, |ip| o.by_address.get(&ip).copied().unwrap_or(0));
        if o.total >= config.max_calls || here >= config.max_calls_per_address {
            return None;
        }
        o.total += 1;
        if let Some(ip) = ip {
            *o.by_address.entry(ip).or_default() += 1;
        }
        Some(Slot { open: open.clone(), ip })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut o = self.open.lock().unwrap_or_else(|e| e.into_inner());
        o.total -= 1;
        if let Some(ip) = self.ip {
            if let Some(n) = o.by_address.get_mut(&ip) {
                *n -= 1;
                if *n == 0 {
                    o.by_address.remove(&ip);
                }
            }
        }
    }
}

impl russh::server::Server for Listener {
    type Handler = Connection;

    fn new_client(&mut self, address: Option<SocketAddr>) -> Connection {
        let slot = Slot::take(&self.open, address, &self.config);
        Connection { board: self.board.clone(), config: self.config.clone(), address, slot, user: None, channels: HashMap::new() }
    }
}

/// One caller's connection, and its session channel.
struct Connection {
    board: Arc<dyn Board>,
    config: Arc<Config>,
    address: Option<SocketAddr>,
    /// `None` when the limits were full as it came: it's turned away.
    slot: Option<Slot>,
    user: Option<String>,
    channels: HashMap<ChannelId, Line>,
}

/// A session channel, before and after its shell starts.
#[derive(Default)]
struct Line {
    term: String,
    size: Size,
    env: Vec<(String, String)>,
    /// Set once the call has started: where its events go.
    events: Option<mpsc::Sender<Event>>,
}

impl Connection {
    fn guest(&mut self, user: &str) -> Result<Auth, russh::Error> {
        if self.slot.is_none() {
            return Err(russh::Error::Disconnect);
        }
        if user == self.config.guest {
            self.user = Some(user.to_string());
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }
}

impl russh::server::Handler for Connection {
    type Error = russh::Error;

    async fn auth_none(&mut self, user: &str) -> Result<Auth, Self::Error> {
        self.guest(user)
    }

    async fn auth_password(&mut self, user: &str, _: &str) -> Result<Auth, Self::Error> {
        self.guest(user)
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: ChannelOpenHandle, _: &mut Session) -> Result<(), Self::Error> {
        if self.channels.is_empty() {
            self.channels.insert(channel.id(), Line::default());
            reply.accept().await;
        }
        Ok(())
    }

    async fn pty_request(&mut self, channel: ChannelId, term: &str, cols: u32, rows: u32, width: u32, height: u32, _: &[(russh::Pty, u32)], session: &mut Session) -> Result<(), Self::Error> {
        if let Some(line) = self.channels.get_mut(&channel) {
            line.term = term.to_string();
            line.size = size(cols, rows, width, height);
        }
        session.channel_success(channel)
    }

    async fn env_request(&mut self, channel: ChannelId, name: &str, value: &str, session: &mut Session) -> Result<(), Self::Error> {
        if let Some(line) = self.channels.get_mut(&channel) {
            line.env.push((name.to_string(), value.to_string()));
        }
        session.channel_success(channel)
    }

    async fn exec_request(&mut self, channel: ChannelId, _: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        session.channel_failure(channel)
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        let Some(line) = self.channels.get_mut(&channel).filter(|l| l.events.is_none()) else { return session.channel_failure(channel) };
        session.channel_success(channel)?;
        let (events, heard) = mpsc::channel();
        let (out, mut said) = unbounded_channel();
        line.events = Some(events);
        let user = self.user.clone().unwrap_or_default();
        let mut call = Call::new(user, self.address, line.term.clone(), line.size, line.env.clone(), heard, out.clone());
        let (board, wait) = (self.board.clone(), self.config.ask);
        std::thread::spawn(move || {
            call.find_caller(wait);
            let done = board.call(&mut call);
            let _ = out.send(Out::End(u32::from(done.is_err())));
        });
        let handle = session.handle();
        tokio::spawn(async move {
            while let Some(sent) = said.recv().await {
                match sent {
                    Out::Data(bytes) => {
                        if handle.data(channel, bytes).await.is_err() {
                            break;
                        }
                    }
                    Out::End(status) => {
                        let _ = handle.exit_status_request(channel, status).await;
                        let _ = handle.eof(channel).await;
                        let _ = handle.close(channel).await;
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], _: &mut Session) -> Result<(), Self::Error> {
        if let Some(events) = self.channels.get(&channel).and_then(|l| l.events.as_ref()) {
            let _ = events.send(Event::Data(data.to_vec()));
        }
        Ok(())
    }

    async fn window_change_request(&mut self, channel: ChannelId, cols: u32, rows: u32, width: u32, height: u32, _: &mut Session) -> Result<(), Self::Error> {
        if let Some(line) = self.channels.get_mut(&channel) {
            line.size = size(cols, rows, width, height);
            if let Some(events) = &line.events {
                let _ = events.send(Event::Resize(line.size));
            }
        }
        Ok(())
    }

    async fn channel_eof(&mut self, channel: ChannelId, _: &mut Session) -> Result<(), Self::Error> {
        // The caller's side is done: dropping where the events go ends the
        // call's keys, so its board sees a hang-up.
        if let Some(line) = self.channels.get_mut(&channel) {
            line.events = None;
        }
        Ok(())
    }

    async fn channel_close(&mut self, channel: ChannelId, _: &mut Session) -> Result<(), Self::Error> {
        self.channels.remove(&channel);
        Ok(())
    }
}

/// A window's size from SSH's numbers, held to what a terminal has.
fn size(cols: u32, rows: u32, width: u32, height: u32) -> Size {
    let cells = |n: u32| n.min(u16::MAX as u32) as u16;
    Size { cols: cells(cols), rows: cells(rows), width, height }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_key_is_made_once_then_kept_for_the_phonebook_to_pin() {
        let dir = std::env::temp_dir().join(format!("flow-bbs-key-{}", std::process::id()));
        let path = dir.join("keys").join("host_key");
        let made = host_key(&path).unwrap();
        let again = host_key(&path).unwrap();
        assert_eq!(made.public_key(), again.public_key());
        assert_eq!(made.algorithm(), Algorithm::Ed25519);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "only its owner may read it");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn connections_past_the_limits_are_turned_away_and_their_places_given_back() {
        let config = Config { max_calls: 3, max_calls_per_address: 2, ..Config::default() };
        let open = Arc::default();
        let at = |n: u8| Some(SocketAddr::from(([10, 0, 0, n], 5000)));
        let a1 = Slot::take(&open, at(1), &config).unwrap();
        let a2 = Slot::take(&open, at(1), &config).unwrap();
        assert!(Slot::take(&open, at(1), &config).is_none(), "two from one address");
        let b = Slot::take(&open, at(2), &config).unwrap();
        assert!(Slot::take(&open, at(3), &config).is_none(), "three in all");
        drop(a1);
        assert!(Slot::take(&open, at(1), &config).is_some(), "a place given back is taken again");
        drop((a2, b));
        assert_eq!(open.lock().unwrap().total, 0);
        assert!(open.lock().unwrap().by_address.is_empty());
    }

    #[test]
    fn a_window_too_big_for_a_terminal_is_held_to_one() {
        assert_eq!(size(80, 24, 800, 480), Size { cols: 80, rows: 24, width: 800, height: 480 });
        assert_eq!(size(1 << 20, 24, 0, 0).cols, u16::MAX);
    }
}
