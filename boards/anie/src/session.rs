//! A call's way through the board: the welcome and the guest's menu, then,
//! once they've logged in, the member's, and what every part of the board
//! shares: who's calling, the database, the log, the mail.

use std::io;
use std::net::IpAddr;

use flow_bbs::{printable, Call, Item};
use postgres::Client;

use crate::db::{self, Db, Role};
use crate::ui::{self, BOLD, DIM, PLAIN};
use crate::Bbs;

/// A call on the board.
pub struct Session<'a> {
    pub bbs: &'a Bbs,
    pub call: &'a mut Call,
    pub db: Client,
    /// The call's number among those on now.
    pub id: u64,
    /// Who they are, once they've logged in.
    pub me: Option<Me>,
}

/// A member logged in.
#[derive(Debug, Clone)]
pub struct Me {
    pub id: i64,
    pub handle: String,
    pub role: Role,
}

/// One call, start to end.
pub fn run(bbs: &Bbs, id: u64, call: &mut Call) -> io::Result<()> {
    let db = match db::connect(&bbs.settings.database_url, &bbs.settings.zone) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("anie-bbs: database: {}", db::class(&e));
            call.say("\n The board can't reach its records just now. Please call again later.\n")?;
            return Ok(());
        }
    };
    let mut s = Session { bbs, call, db, id, me: None };
    s.guest()
}

impl Session<'_> {
    /// What they're doing, for who's on.
    pub fn doing(&self, what: &'static str) {
        if let Some(o) = self.bbs.online.lock().unwrap().get_mut(&self.id) {
            o.doing = what;
            o.handle = self.me.as_ref().map(|m| m.handle.clone());
        }
    }

    /// Where they're calling from.
    pub fn ip(&self) -> IpAddr {
        self.call.address().map_or(IpAddr::from([0, 0, 0, 0]), |a| a.ip())
    }

    /// The member logged in. Only the member's parts of the board call it.
    pub fn me(&self) -> &Me {
        self.me.as_ref().expect("a member's part of the board, before logging in")
    }

    /// Whether they're at least `role`.
    pub fn is(&self, role: Role) -> bool {
        self.me.as_ref().is_some_and(|m| m.role >= role)
    }

    /// Writes down what happened, by whom (the member logged in, unless
    /// `actor` says), from where.
    pub fn audit(&mut self, actor: Option<i64>, action: &str, subject: &str) -> io::Result<()> {
        let actor = actor.or(self.me.as_ref().map(|m| m.id));
        let address = self.ip().to_string();
        let subject = ui::cut(subject, 200);
        self.db.execute("INSERT INTO audit (actor_id, action, subject, address) VALUES ($1, $2, $3, $4)", &[&actor, &action, &subject, &address]).db()?;
        Ok(())
    }

    /// Sends mail, and says in the log (without the address) if it didn't go.
    pub fn mail(&self, to: &str, subject: &str, body: &str) -> bool {
        match self.bbs.mailer.send(to, subject, body) {
            Ok(()) => true,
            Err(class) => {
                eprintln!("anie-bbs: mail \"{subject}\" didn't go: {class}");
                false
            }
        }
    }

    /// Mails the sysops: to the board's sysop address where it has one,
    /// else to each active sysop's own.
    pub fn tell_sysops(&mut self, subject: &str, body: &str) -> io::Result<()> {
        let to: Vec<String> = match &self.bbs.settings.sysop_mail {
            Some(a) => vec![a.clone()],
            None => self.db.query("SELECT email FROM members WHERE role = 'sysop' AND status = 'active'", &[]).db()?.iter().map(|r| r.get(0)).collect(),
        };
        for address in to {
            self.mail(&address, subject, body);
        }
        Ok(())
    }

    /// `pause`, for after something's been said: `false` once they hang up.
    pub fn pause(&mut self) -> io::Result<bool> {
        self.call.pause(" Press any key to go on.")
    }

    /// The guest's menu: log in, apply, the rest.
    fn guest(&mut self) -> io::Result<()> {
        let items = [
            Item::new('l', "Log in"),
            Item::new('a', "Apply for membership"),
            Item::new('f', "Forgot your password"),
            Item::new('i', "About this board"),
            Item::new('g', "Game room (experimental)"),
            Item::new('d', "Demonstrations: what Flow can show"),
            Item::new('w', "Who's on"),
            Item::new('q', "Hang up"),
        ];
        let name = self.bbs.settings.name.clone();
        loop {
            self.doing("At the welcome");
            let values = [("board", name.as_str())];
            let chosen = self.call.menu_titled(Some(&self.bbs.screens.welcome), &values, "Welcome", &items)?;
            match chosen {
                None => return Ok(()),
                Some(0) => {
                    if self.login()? {
                        return self.member();
                    }
                }
                Some(1) => self.apply()?,
                Some(2) => self.forgot()?,
                Some(3) => {
                    self.call.show(&self.bbs.screens.about, &values)?;
                    self.pause()?;
                }
                Some(4) => self.games()?,
                Some(5) => self.demos()?,
                Some(6) => self.whos_on()?,
                _ => {
                    self.call.say(" Goodbye.\n")?;
                    return Ok(());
                }
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// The member's menu, after the bulletin.
    fn member(&mut self) -> io::Result<()> {
        self.bulletin()?;
        loop {
            if self.call.hung_up() {
                return Ok(());
            }
            self.doing("At the main menu");
            let me = self.me().clone();
            let mut items = vec![
                Item::new('f', "Forums"),
                Item::new('b', "Bug tracker"),
                Item::new('g', "Game room (experimental)"),
                Item::new('d', "Demonstrations"),
                Item::new('w', "Who's on"),
                Item::new('n', "The bulletin"),
                Item::new('p', "Profile and security"),
            ];
            if me.role == Role::Sysop {
                items.push(Item::new('s', "Sysop"));
            }
            items.push(Item::new('q', "Hang up"));
            let values = [("board", self.bbs.settings.name.as_str()), ("handle", me.handle.as_str()), ("role", me.role.as_str())];
            let Some(chosen) = self.call.menu_titled(Some(&self.bbs.screens.main), &values, "Main menu", &items)? else { return Ok(()) };
            match items[chosen].key {
                'f' => self.forums()?,
                'b' => self.tracker()?,
                'g' => self.games()?,
                'd' => self.demos()?,
                'w' => self.whos_on()?,
                'n' => self.bulletin()?,
                'p' => self.profile()?,
                's' if self.is(Role::Sysop) => self.sysop()?,
                _ => {
                    self.call.say(" Goodbye.\n")?;
                    return Ok(());
                }
            }
        }
    }

    /// The bulletin, and what's new since they were last on.
    fn bulletin(&mut self) -> io::Result<()> {
        let me = self.me().clone();
        let bulletin: String = self.db.query_opt("SELECT value FROM settings WHERE key = 'bulletin'", &[]).db()?.map(|r| r.get(0)).unwrap_or_default();
        let roles = me.role.and_below();
        let new = self
            .db
            .query_one(
                "SELECT count(*) FROM threads t JOIN forums f ON f.id = t.forum_id LEFT JOIN thread_reads r ON r.thread_id = t.id AND r.member_id = $1 \
                 WHERE NOT t.deleted AND NOT f.archived AND f.read_role = ANY($2) AND (r.last_post_id IS NULL OR r.last_post_id < t.last_post_id)",
                &[&me.id, &roles],
            )
            .db()?
            .get::<_, i64>(0);
        let mine = self.db.query_one("SELECT count(*) FROM bugs WHERE assignee_id = $1 AND status IN ('new', 'confirmed', 'in-progress')", &[&me.id]).db()?.get::<_, i64>(0);
        let (cols, _) = ui::window(self.call);
        let mut lines = ui::wrap(&bulletin, cols.saturating_sub(4));
        lines.push(String::new());
        lines.push(format!("{BOLD}{new}{PLAIN} thread{} with posts you haven't read.", if new == 1 { "" } else { "s" }));
        if mine > 0 {
            lines.push(format!("{BOLD}{mine}{PLAIN} open bug{} assigned to you.", if mine == 1 { "" } else { "s" }));
        }
        if self.is(Role::Sysop) {
            let waiting = self.db.query_one("SELECT count(*) FROM members WHERE status = 'pending'", &[]).db()?.get::<_, i64>(0);
            if waiting > 0 {
                lines.push(format!("{BOLD}{waiting}{PLAIN} application{} waiting for you in the sysop menu.", if waiting == 1 { "" } else { "s" }));
            }
        }
        if ui::page(self.call, "The bulletin", &lines)? {
            self.pause()?;
        }
        Ok(())
    }

    /// Who's on now, and what they're doing; the sysop sees from where.
    fn whos_on(&mut self) -> io::Result<()> {
        let on: Vec<_> = self.bbs.online.lock().unwrap().values().cloned().collect();
        let sysop = self.is(Role::Sysop);
        let mut lines = Vec::new();
        for o in on {
            let who = o.handle.map_or(format!("{DIM}{}{PLAIN}", ui::pad("a guest", 20)), |h| ui::pad(&h, 20));
            let minutes = o.since.elapsed().as_secs() / 60;
            let from = if sysop { format!("  {DIM}{}{PLAIN}", printable(&o.address)) } else { String::new() };
            lines.push(format!("{who}  {}  {DIM}{minutes} min{PLAIN}{from}", ui::pad(o.doing, 18)));
        }
        if ui::page(self.call, "Who's on", &lines)? {
            self.pause()?;
        }
        Ok(())
    }
}
