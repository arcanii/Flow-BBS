//! The Anie Solutions BBS: a board in code on Flow's BBS SDK, its members
//! let in by the sysop, with forums and a bug tracker, in PostgreSQL.
//!
//! A caller comes in as the guest and applies: a handle, their name, an
//! email address (its code mailed to prove it), a password, and an
//! authenticator app set up from a QR code. Then the sysop decides. A
//! member logs in with their handle, password and the app's code (or a
//! recovery code, once each). Roles go member < staff < sysop: staff
//! moderate the forums and triage bugs; the sysop runs the board from its
//! sysop menu (applications, members and their roles, forums, products,
//! the bulletin, the log). Guests and members alike have the game room
//! (experimental): Mines (the door `flow-mines`), its members' best times
//! a leaderboard they share, and Neko (the door `flow-cat`); and the
//! demonstrations of what Flow draws: a gallery, Newell's teapot, a
//! hypercube, a room lit by lamps of its own, and a page of type.

pub mod crypto;
pub mod db;
pub mod mail;
pub mod settings;
pub mod ui;

mod account;
mod demo;
mod forums;
mod games;
mod models;
mod session;
mod sysop;
mod tracker;

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::net::{IpAddr, TcpListener};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use flow_bbs::{Call, Config, Door, Screen};

use crate::mail::Mailer;
use crate::settings::Settings;

/// The board, shared by every call.
pub struct Bbs {
    pub settings: Settings,
    pub mailer: Box<dyn Mailer>,
    pub screens: Screens,
    /// Neko's door, for the game room.
    pub neko: Door,
    /// Who's on now, by call.
    pub online: Mutex<BTreeMap<u64, Online>>,
    /// What's been tried from where, lately.
    pub guard: Mutex<Guard>,
    next_call: AtomicU64,
}

/// The screens from the board's folder, the demonstrations' models made in
/// code (`models`).
pub struct Screens {
    pub welcome: Screen,
    /// Over a member's menu.
    pub main: Screen,
    pub about: Screen,
    pub rules: Screen,
    pub games: Screen,
    /// Over the demonstrations' menu, and each of them.
    pub demos: Screen,
    pub gallery: Screen,
    pub teapot: Screen,
    pub hypercube: Screen,
    pub lamps: Screen,
    pub type_page: Screen,
}

impl Screens {
    pub fn read(folder: &std::path::Path) -> io::Result<Screens> {
        let made = models::made();
        let made: Vec<(&str, &[u8])> = made.iter().map(|(name, bytes)| (*name, bytes.as_slice())).collect();
        let one = |name: &str| Screen::read_with(&folder.join(format!("{name}.txt")), folder, &made);
        Ok(Screens {
            welcome: one("welcome")?,
            main: one("main")?,
            about: one("about")?,
            rules: one("rules")?,
            games: one("games")?,
            demos: one("demos")?,
            gallery: one("gallery")?,
            teapot: one("teapot")?,
            hypercube: one("hypercube")?,
            lamps: one("lamps")?,
            type_page: one("type")?,
        })
    }
}

/// Someone on the board now.
#[derive(Debug, Clone)]
pub struct Online {
    pub handle: Option<String>,
    pub doing: &'static str,
    pub since: Instant,
    pub address: String,
}

/// Login failures and applications by address, kept an hour: what the
/// limits are counted from.
#[derive(Default)]
pub struct Guard {
    failures: HashMap<IpAddr, Vec<Instant>>,
    applications: HashMap<IpAddr, Vec<Instant>>,
}

/// Login failures from one address in 15 minutes before it's told to wait.
pub const FAILURES_PER_ADDRESS: usize = 10;
/// Applications from one address in an hour.
pub const APPLICATIONS_PER_ADDRESS: usize = 3;

impl Guard {
    fn count(list: &mut HashMap<IpAddr, Vec<Instant>>, ip: IpAddr, within: Duration) -> usize {
        let now = Instant::now();
        list.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < Duration::from_secs(3600));
            !times.is_empty()
        });
        list.get(&ip).map_or(0, |times| times.iter().filter(|t| now.duration_since(**t) < within).count())
    }

    pub fn failures(&mut self, ip: IpAddr) -> usize {
        Self::count(&mut self.failures, ip, Duration::from_secs(15 * 60))
    }

    pub fn failed(&mut self, ip: IpAddr) {
        self.failures.entry(ip).or_default().push(Instant::now());
    }

    pub fn applications(&mut self, ip: IpAddr) -> usize {
        Self::count(&mut self.applications, ip, Duration::from_secs(3600))
    }

    pub fn applied(&mut self, ip: IpAddr) {
        self.applications.entry(ip).or_default().push(Instant::now());
    }
}

impl Bbs {
    /// The board, its screens read from its folder.
    pub fn new(settings: Settings, mailer: Box<dyn Mailer>) -> io::Result<Bbs> {
        let screens = Screens::read(&settings.screens)?;
        let neko = Door::new(settings.neko.clone(), Vec::<String>::new());
        Ok(Bbs { settings, mailer, screens, neko, online: Mutex::default(), guard: Mutex::default(), next_call: AtomicU64::new(1) })
    }

    /// The SDK's settings for serving it.
    pub fn config(&self) -> Config {
        Config {
            address: self.settings.address.clone(),
            host_key: self.settings.host_key.clone(),
            idle: Some(Duration::from_secs(30 * 60)),
            max_calls: self.settings.max_calls,
            max_calls_per_address: self.settings.max_calls_per_address,
            ..Config::default()
        }
    }

    /// Brings the database's schema up to date.
    pub fn migrate(&self) -> Result<usize, postgres::Error> {
        let mut client = db::connect(&self.settings.database_url, &self.settings.zone)?;
        db::migrate(&mut client)
    }

    /// One call, from the welcome to hang-up.
    pub fn call(&self, call: &mut Call) -> io::Result<()> {
        let id = self.next_call.fetch_add(1, Ordering::Relaxed);
        let address = call.address().map(|a| a.ip().to_string()).unwrap_or_default();
        self.online.lock().unwrap().insert(id, Online { handle: None, doing: "Arriving", since: Instant::now(), address });
        let done = session::run(self, id, call);
        self.online.lock().unwrap().remove(&id);
        if let Err(e) = &done {
            if e.get_ref().is_some_and(|e| e.is::<db::Broke>()) && !call.hung_up() {
                let _ = call.say("\n Something went wrong on the board's side, and it's been noted. Please call again later.\n");
            }
        }
        done
    }
}

/// Serves `bbs` until the process ends, on its address or on `listener`.
pub fn serve(bbs: Bbs, listener: Option<TcpListener>) -> io::Result<()> {
    let config = bbs.config();
    let bbs = Arc::new(bbs);
    let board = move |call: &mut Call| bbs.call(call);
    match listener {
        Some(l) => flow_bbs::serve_on(l, config, board),
        None => flow_bbs::serve(config, board),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_counts_what_was_tried_from_each_address_apart() {
        let mut g = Guard::default();
        let (a, b): (IpAddr, IpAddr) = ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap());
        for _ in 0..3 {
            g.failed(a);
        }
        g.applied(b);
        assert_eq!((g.failures(a), g.failures(b)), (3, 0));
        assert_eq!((g.applications(a), g.applications(b)), (0, 1));
    }
}
