//! The sysop's menu: applications to decide, members and their roles,
//! forums and the tracker's products, the bulletin, and the log of what
//! happened. Every decision and change is written to the log, and a member
//! decided on is told by mail.
//!
//! A sysop can't demote, suspend or reset themselves from here, so the
//! board can't be left with no one to run it by a slip of the keys.

use std::io;

use flow_bbs::{printable, Item};

use crate::db::{Db, Role};
use crate::forums::{pick, Pick};
use crate::session::Session;
use crate::ui::{self, BOLD, CYAN, DIM, PLAIN, YELLOW};

const ROLES: [&str; 3] = ["member", "staff", "sysop"];

impl Session<'_> {
    pub fn sysop(&mut self) -> io::Result<()> {
        let items = [
            Item::new('a', "Applications"),
            Item::new('m', "Members"),
            Item::new('f', "Forums"),
            Item::new('p', "Products in the bug tracker"),
            Item::new('n', "The bulletin"),
            Item::new('l', "The log"),
            Item::new('b', "Back"),
        ];
        loop {
            if !self.is(Role::Sysop) || self.call.hung_up() {
                return Ok(());
            }
            self.doing("In the sysop menu");
            let waiting: i64 = self.db.query_one("SELECT count(*) FROM members WHERE status = 'pending'", &[]).db()?.get(0);
            let top = format!("{{bold}}{{bright-cyan}}Sysop{{/}}\n{{dim}}{waiting} application(s) waiting{{/}}");
            let screen = flow_bbs::Screen::parse(&top, &self.bbs.settings.screens).map_err(io::Error::other)?;
            let Some(chosen) = self.call.menu_titled(Some(&screen), &[], "Sysop menu", &items)? else { return Ok(()) };
            match items[chosen].key {
                'a' => self.applications()?,
                'm' => self.members()?,
                'f' => self.forum_admin()?,
                'p' => self.products()?,
                'n' => self.edit_bulletin()?,
                'l' => self.log()?,
                _ => return Ok(()),
            }
        }
    }

    /// The applications waiting, oldest first, each to approve or turn down.
    fn applications(&mut self) -> io::Result<()> {
        loop {
            let rows = self
                .db
                .query("SELECT id, handle, real_name, email, to_char(created_at, 'YYYY-MM-DD HH24:MI'), applied_from FROM members WHERE status = 'pending' ORDER BY created_at LIMIT 50", &[])
                .db()?;
            ui::heading(self.call, "Applications")?;
            if rows.is_empty() {
                self.call.say(&format!(" {DIM}None waiting.{PLAIN}\n"))?;
                self.pause()?;
                return Ok(());
            }
            for (i, r) in rows.iter().enumerate() {
                let (handle, real, email, at, from): (String, String, String, String, String) = (r.get(1), r.get(2), r.get(3), r.get(4), r.get(5));
                self.call.say(&format!(" {CYAN}{:>2}{PLAIN} {BOLD}{}{PLAIN} {} {}\n    {DIM}applied {at} from {}{PLAIN}\n", i + 1, ui::pad(&handle, 20), ui::pad(&real, 24), ui::cut(&email, 30), printable(&from)))?;
            }
            let Some(typed) = ui::answer(self.call, "\n Number to decide, Q to go back: ", 3)? else { return Ok(()) };
            match pick(&typed) {
                Pick::Number(n) if (1..=rows.len()).contains(&n) => {
                    let (id, handle, email): (i64, String, String) = (rows[n - 1].get(0), rows[n - 1].get(1), rows[n - 1].get(3));
                    let Some(a) = ui::answer(self.call, &format!(" {handle}: A to approve, R to turn down, Enter to leave it: "), 1)? else { continue };
                    match a.to_ascii_lowercase().as_str() {
                        "a" => self.decide(id, &handle, &email, "active", "")?,
                        "r" => {
                            let Some(why) = ui::answer(self.call, " Why (they're told; Enter for no reason): ", 200)? else { continue };
                            self.decide(id, &handle, &email, "rejected", &why)?;
                        }
                        _ => {}
                    }
                }
                Pick::Letter('q') => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// Approves (`active`) or turns down (`rejected`) application `id`, and
    /// tells them.
    fn decide(&mut self, id: i64, handle: &str, email: &str, status: &str, why: &str) -> io::Result<()> {
        let me = self.me().id;
        let n = self.db.execute("UPDATE members SET status = $2, status_note = $3, decided_at = now(), decided_by = $4 WHERE id = $1 AND status = 'pending'", &[&id, &status, &why, &me]).db()?;
        if n == 0 {
            return Ok(());
        }
        let name = self.bbs.settings.name.clone();
        if status == "active" {
            self.audit(None, "application approved", handle)?;
            self.mail(email, &format!("{name}: welcome, {handle}"), &format!("Your application to {name} has been approved. Call the board and log in with your handle, {handle}, your password and your authenticator's code.\n\nWelcome!"));
            ui::done(self.call, &format!("{handle} is a member now, and has been told."))?;
        } else {
            self.audit(None, "application turned down", handle)?;
            let why = if why.is_empty() { String::new() } else { format!("\n\nThe sysop says: {why}") };
            self.mail(email, &format!("{name}: your application"), &format!("Your application to {name} wasn't accepted.{why}"));
            ui::done(self.call, &format!("{handle} was turned down, and has been told."))?;
        }
        self.pause()?;
        Ok(())
    }

    /// The members, a page at a time, found by handle.
    fn members(&mut self) -> io::Result<()> {
        let mut find = String::new();
        let mut page = 0i64;
        loop {
            let (_, rows) = ui::window(self.call);
            let per = rows.saturating_sub(7).max(5) as i64;
            let like = format!("%{}%", find.to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            let total: i64 = self.db.query_one("SELECT count(*) FROM members WHERE handle_key LIKE $1", &[&like]).db()?.get(0);
            let pages = ((total + per - 1) / per).max(1);
            page = page.clamp(0, pages - 1);
            let list = self
                .db
                .query(
                    "SELECT id, handle, role, status, coalesce(to_char(last_login_at, 'YYYY-MM-DD'), 'never') FROM members WHERE handle_key LIKE $1 ORDER BY handle_key LIMIT $2 OFFSET $3",
                    &[&like, &per, &(page * per)],
                )
                .db()?;
            let found = if find.is_empty() { String::new() } else { format!(", handles with \"{find}\"") };
            ui::heading(self.call, &format!("Members{found} (page {} of {pages})", page + 1))?;
            for (i, r) in list.iter().enumerate() {
                let (handle, role, status, last): (String, String, String, String) = (r.get(1), r.get(2), r.get(3), r.get(4));
                let status = if status == "active" { status } else { format!("{YELLOW}{status}{PLAIN}") };
                self.call.say(&format!(" {CYAN}{:>3}{PLAIN} {} {} {}  {DIM}last on {last}{PLAIN}\n", i + 1, ui::pad(&handle, 20), ui::pad(&role, 7), status))?;
            }
            let Some(typed) = ui::answer(self.call, "\n Number to look at, / to find by handle, ] next page, [ last page, Q to go back: ", 20)? else { return Ok(()) };
            if let Some(f) = typed.strip_prefix('/') {
                find = f.trim().to_string();
                page = 0;
                continue;
            }
            match pick(&typed) {
                Pick::Number(n) if (1..=list.len()).contains(&n) => self.member_card(list[n - 1].get(0))?,
                Pick::Letter(']') => page += 1,
                Pick::Letter('[') => page -= 1,
                Pick::Letter('q') => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// One member, and what the sysop can do to them.
    fn member_card(&mut self, id: i64) -> io::Result<()> {
        loop {
            let r = self
                .db
                .query_one(
                    "SELECT handle, real_name, email, email_verified, role, status, status_note, to_char(created_at, 'YYYY-MM-DD HH24:MI'), \
                     coalesce(to_char(last_login_at, 'YYYY-MM-DD HH24:MI'), 'never'), seed_sealed IS NOT NULL, coalesce(locked_until > now(), false), \
                     (SELECT count(*) FROM posts WHERE author_id = $1), (SELECT count(*) FROM bugs WHERE reporter_id = $1) FROM members WHERE id = $1",
                    &[&id],
                )
                .db()?;
            let (handle, real, email, verified, role, status, note, since, last): (String, String, String, bool, String, String, String, String, String) =
                (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6), r.get(7), r.get(8));
            let (has_app, locked, posts, bugs): (bool, bool, i64, i64) = (r.get(9), r.get(10), r.get(11), r.get(12));
            ui::heading(self.call, &format!("Member: {handle}"))?;
            self.call.say(&format!(
                " Name          {}\n Email         {}{}\n Role          {BOLD}{role}{PLAIN}\n Status        {status}{}\n Applied       {since}\n Last on       {last}\n Authenticator {}{}\n Posts {posts}, bugs filed {bugs}\n\n",
                printable(&real),
                printable(&email),
                if verified { "" } else { " (not proved)" },
                if note.is_empty() { String::new() } else { format!(" ({})", printable(&note)) },
                if has_app { "set up" } else { "none" },
                if locked { format!("   {YELLOW}waiting out failed logins{PLAIN}") } else { String::new() },
            ))?;
            let me = self.me().id;
            let own = id == me;
            let mut actions = vec!["R role"];
            if status == "suspended" {
                actions.push("U reinstate");
            } else if status == "active" {
                actions.push("S suspend");
            }
            actions.extend(["A reset authenticator", "L lift the login wait", "B back"]);
            if own {
                actions = vec!["B back (a sysop can't change themselves here)"];
            }
            let Some(typed) = ui::answer(self.call, &format!(" {}: ", actions.join(", ")), 1)? else { return Ok(()) };
            match (typed.to_ascii_lowercase().as_str(), own) {
                ("r", false) => {
                    let Some(to) = self.choose(" Their role? ", &ROLES, &role)? else { continue };
                    if to != role {
                        self.db.execute("UPDATE members SET role = $2 WHERE id = $1", &[&id, &to]).db()?;
                        self.audit(None, "role changed", &format!("{handle}: {role} → {to}"))?;
                    }
                }
                ("s", false) if status == "active" => {
                    let Some(why) = ui::answer(self.call, " Why (they're told when they log in): ", 200)? else { continue };
                    self.db.execute("UPDATE members SET status = 'suspended', status_note = $2 WHERE id = $1", &[&id, &why]).db()?;
                    self.audit(None, "member suspended", &handle)?;
                }
                ("u", false) if status == "suspended" => {
                    self.db.execute("UPDATE members SET status = 'active', status_note = '' WHERE id = $1", &[&id]).db()?;
                    self.audit(None, "member reinstated", &handle)?;
                }
                ("a", false) => {
                    if ui::yes(self.call, &format!(" Reset {handle}'s authenticator? They'll set up a new one after a code to their email"))? == Some(true) {
                        let mut t = self.db.transaction().db()?;
                        t.execute("UPDATE members SET seed_sealed = NULL, last_step = NULL WHERE id = $1", &[&id]).db()?;
                        t.execute("DELETE FROM recovery_codes WHERE member_id = $1", &[&id]).db()?;
                        t.commit().db()?;
                        self.audit(None, "authenticator reset", &handle)?;
                        let name = self.bbs.settings.name.clone();
                        self.mail(&email, &format!("{name}: your authenticator was reset"), &format!("The sysop reset the authenticator for {handle} on {name}. Next time you log in, use your handle and password, leave the code empty, and a code will come here before you set up a new one.\n\nIf you didn't ask for this, tell the sysop."));
                    }
                }
                ("l", false) => {
                    self.db.execute("UPDATE members SET locked_until = NULL, failed_logins = 0 WHERE id = $1", &[&id]).db()?;
                    self.audit(None, "login wait lifted", &handle)?;
                }
                ("b", _) => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// The forums: new ones, and each one's name, words, roles, place and
    /// whether it's archived.
    fn forum_admin(&mut self) -> io::Result<()> {
        loop {
            let rows = self.db.query("SELECT id, name, description, read_role, post_role, position, archived FROM forums ORDER BY position, name", &[]).db()?;
            ui::heading(self.call, "Forums")?;
            for (i, r) in rows.iter().enumerate() {
                let (name, read, post, position, archived): (String, String, String, i32, bool) = (r.get(1), r.get(3), r.get(4), r.get(5), r.get(6));
                let archived = if archived { format!("  {YELLOW}archived{PLAIN}") } else { String::new() };
                self.call.say(&format!(" {CYAN}{:>2}{PLAIN} {} {DIM}read {read}, post {post}, place {position}{PLAIN}{archived}\n", i + 1, ui::pad(&name, 24)))?;
            }
            let Some(typed) = ui::answer(self.call, "\n Number to change, N for a new forum, Q to go back: ", 3)? else { return Ok(()) };
            match pick(&typed) {
                Pick::Number(n) if (1..=rows.len()).contains(&n) => {
                    let r = &rows[n - 1];
                    let (id, name, description, read, post, position, archived): (i64, String, String, String, String, i32, bool) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5), r.get(6));
                    self.edit_forum(Some(id), name, description, read, post, position, archived)?;
                }
                Pick::Letter('n') => {
                    let next: i32 = self.db.query_one("SELECT coalesce(max(position), -1) + 1 FROM forums", &[]).db()?.get(0);
                    self.edit_forum(None, String::new(), String::new(), "member".into(), "member".into(), next, false)?;
                }
                Pick::Letter('q') => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// Asks each of a forum's settings in turn, Enter keeping what it has.
    #[allow(clippy::too_many_arguments)]
    fn edit_forum(&mut self, id: Option<i64>, name: String, description: String, read: String, post: String, position: i32, archived: bool) -> io::Result<()> {
        self.call.say(&format!(" {DIM}Enter keeps what's in brackets.{PLAIN}\n"))?;
        let Some(new_name) = ui::answer(self.call, &format!(" Name [{}]: ", printable(&name)), 40)? else { return Ok(()) };
        let name = if new_name.is_empty() { name } else { new_name };
        if name.chars().count() < 2 {
            ui::oops(self.call, "A forum needs a name.")?;
            self.pause()?;
            return Ok(());
        }
        let Some(new_description) = ui::answer(self.call, &format!(" What it's for [{}]: ", ui::cut(&description, 40)), 120)? else { return Ok(()) };
        let description = if new_description.is_empty() { description } else { new_description };
        let Some(read) = self.choose(" Who reads it? ", &ROLES, &read)? else { return Ok(()) };
        let Some(post) = self.choose(" Who posts in it? ", &ROLES, &post)? else { return Ok(()) };
        let Some(place) = ui::answer(self.call, &format!(" Its place in the list [{position}]: "), 4)? else { return Ok(()) };
        let position = place.parse().unwrap_or(position);
        let archived = match id {
            Some(_) => ui::yes(self.call, &format!(" Archived (hidden from members)? Now {}.", if archived { "yes" } else { "no" }))?.unwrap_or(archived),
            None => false,
        };
        let done = match id {
            Some(id) => self.db.execute(
                "UPDATE forums SET name = $2, description = $3, read_role = $4, post_role = $5, position = $6, archived = $7 WHERE id = $1",
                &[&id, &name, &description, &read, &post, &position, &archived],
            ),
            None => self.db.execute("INSERT INTO forums (name, description, read_role, post_role, position) VALUES ($1, $2, $3, $4, $5)", &[&name, &description, &read, &post, &position]),
        };
        match done {
            Ok(_) => self.audit(None, if id.is_some() { "forum changed" } else { "forum added" }, &name)?,
            Err(e) if crate::db::taken(&e) => {
                ui::oops(self.call, "There's a forum of that name already.")?;
                self.pause()?;
            }
            Err(e) => return Err(crate::db::broke(e)),
        }
        Ok(())
    }

    /// The products bugs are filed against.
    fn products(&mut self) -> io::Result<()> {
        loop {
            let rows = self.db.query("SELECT id, name, description, archived FROM products ORDER BY name", &[]).db()?;
            ui::heading(self.call, "Products in the bug tracker")?;
            for (i, r) in rows.iter().enumerate() {
                let (name, description, archived): (String, String, bool) = (r.get(1), r.get(2), r.get(3));
                let archived = if archived { format!("  {YELLOW}archived{PLAIN}") } else { String::new() };
                self.call.say(&format!(" {CYAN}{:>2}{PLAIN} {} {DIM}{}{PLAIN}{archived}\n", i + 1, ui::pad(&name, 24), ui::cut(&description, 40)))?;
            }
            let Some(typed) = ui::answer(self.call, "\n Number to archive or bring back, N for a new product, Q to go back: ", 3)? else { return Ok(()) };
            match pick(&typed) {
                Pick::Number(n) if (1..=rows.len()).contains(&n) => {
                    let (id, name): (i64, String) = (rows[n - 1].get(0), rows[n - 1].get(1));
                    self.db.execute("UPDATE products SET archived = NOT archived WHERE id = $1", &[&id]).db()?;
                    self.audit(None, "product archived or brought back", &name)?;
                }
                Pick::Letter('n') => {
                    let Some(name) = ui::answer(self.call, " Its name: ", 40)? else { continue };
                    let Some(description) = ui::answer(self.call, " In a line, what it is: ", 120)? else { continue };
                    if name.chars().count() >= 2 {
                        match self.db.execute("INSERT INTO products (name, description) VALUES ($1, $2)", &[&name, &description]) {
                            Ok(_) => self.audit(None, "product added", &name)?,
                            Err(e) if crate::db::taken(&e) => {
                                ui::oops(self.call, "There's a product of that name already.")?;
                                self.pause()?;
                            }
                            Err(e) => return Err(crate::db::broke(e)),
                        }
                    }
                }
                Pick::Letter('q') => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// The bulletin members see after logging in.
    fn edit_bulletin(&mut self) -> io::Result<()> {
        let now: String = self.db.query_opt("SELECT value FROM settings WHERE key = 'bulletin'", &[]).db()?.map(|r| r.get(0)).unwrap_or_default();
        ui::heading(self.call, "The bulletin")?;
        if let Some(text) = ui::editor(self.call, &now)? {
            self.db.execute("INSERT INTO settings (key, value) VALUES ('bulletin', $1) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value", &[&text]).db()?;
            self.audit(None, "bulletin changed", "")?;
        }
        Ok(())
    }

    /// The log, newest first.
    fn log(&mut self) -> io::Result<()> {
        let rows = self
            .db
            .query("SELECT to_char(a.at, 'MM-DD HH24:MI:SS'), coalesce(m.handle, '-'), a.action, a.subject, a.address FROM audit a LEFT JOIN members m ON m.id = a.actor_id ORDER BY a.id DESC LIMIT 500", &[])
            .db()?;
        let (cols, _) = ui::window(self.call);
        let lines: Vec<String> = rows
            .iter()
            .map(|r| {
                let (at, who, action, subject, address): (String, String, String, String, String) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
                let line = format!("{at} {} {} {} {address}", ui::pad(&who, 12), ui::pad(&action, 24), ui::cut(&subject, 30));
                ui::cut(&line, cols.saturating_sub(3))
            })
            .collect();
        if ui::page(self.call, "The log (newest first)", &lines)? {
            self.pause()?;
        }
        Ok(())
    }
}
