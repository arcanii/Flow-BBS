//! The bug tracker: bugs filed against a product, each with a severity and a
//! status, and what's said and changed on it. Any member files bugs and
//! comments on them; staff triage (status, severity, who it's assigned to,
//! its title), each change written on the bug as a comment and mailed to
//! whoever filed it.

use std::io;

use flow_bbs::{printable, Call, Preview};

use crate::db::{Db, Role};
use crate::forums::{pick, Pick};
use crate::session::Session;
use crate::ui::{self, BOLD, CYAN, DIM, PLAIN, RED, YELLOW};

pub const SEVERITIES: [&str; 4] = ["low", "normal", "high", "critical"];
pub const STATUSES: [&str; 6] = ["new", "confirmed", "in-progress", "resolved", "closed", "wontfix"];
const OPEN: [&str; 3] = ["new", "confirmed", "in-progress"];

/// Which bugs the list shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Show {
    Open,
    Mine,
    All,
}

impl Show {
    fn next(self) -> Show {
        match self {
            Show::Open => Show::Mine,
            Show::Mine => Show::All,
            Show::All => Show::Open,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Show::Open => "open bugs",
            Show::Mine => "bugs you filed or are assigned",
            Show::All => "all bugs",
        }
    }
}

/// A severity in its color.
fn severity(s: &str) -> String {
    match s {
        "critical" => format!("{RED}{s}{PLAIN}"),
        "high" => format!("{YELLOW}{s}{PLAIN}"),
        _ => s.to_string(),
    }
}

impl Session<'_> {
    /// The tracker's list, a page at a time.
    pub fn tracker(&mut self) -> io::Result<()> {
        let (mut show, mut page) = (Show::Open, 0i64);
        loop {
            self.doing("In the bug tracker");
            let me = self.me().clone();
            let (cols, rows) = ui::window(self.call);
            let per = rows.saturating_sub(6).max(5) as i64;
            let open: Vec<String> = OPEN.iter().map(|s| s.to_string()).collect();
            let all: Vec<String> = STATUSES.iter().map(|s| s.to_string()).collect();
            let (statuses, mine) = match show {
                Show::Open => (&open, None),
                Show::Mine => (&all, Some(me.id)),
                Show::All => (&all, None),
            };
            let filter = "b.status = ANY($1) AND ($2::bigint IS NULL OR b.reporter_id = $2 OR b.assignee_id = $2)";
            let total: i64 = self.db.query_one(&format!("SELECT count(*) FROM bugs b WHERE {filter}"), &[statuses, &mine]).db()?.get(0);
            let pages = ((total + per - 1) / per).max(1);
            page = page.clamp(0, pages - 1);
            let bugs = self
                .db
                .query(
                    &format!(
                        "SELECT b.id, b.title, b.severity, b.status, p.name, a.handle FROM bugs b JOIN products p ON p.id = b.product_id LEFT JOIN members a ON a.id = b.assignee_id \
                         WHERE {filter} ORDER BY b.updated_at DESC LIMIT $3 OFFSET $4"
                    ),
                    &[statuses, &mine, &per, &(page * per)],
                )
                .db()?;
            ui::heading(self.call, &format!("Bug tracker: {} (page {} of {pages})", show.name(), page + 1))?;
            if bugs.is_empty() {
                self.call.say(&format!(" {DIM}None.{PLAIN}\n"))?;
            }
            let title_width = cols.saturating_sub(50).max(10);
            for b in &bugs {
                let (id, title, sev, status, product, assignee): (i64, String, String, String, String, Option<String>) = (b.get(0), b.get(1), b.get(2), b.get(3), b.get(4), b.get(5));
                let sev = severity(&ui::pad(&sev, 8));
                self.call.say(&format!(
                    " {CYAN}#{id:<5}{PLAIN} {sev} {} {} {} {DIM}{}{PLAIN}\n",
                    ui::pad(&status, 11),
                    ui::pad(&product, 12),
                    ui::pad(&title, title_width),
                    ui::cut(&assignee.unwrap_or_default(), 12)
                ))?;
            }
            let mut prompt = String::from("\n A bug's # to read, N to file one, F for ");
            prompt.push_str(show.next().name());
            if pages > 1 {
                prompt.push_str(", ] next page, [ last page");
            }
            prompt.push_str(", Q to go back: ");
            let Some(typed) = ui::answer(self.call, &prompt, 8)? else { return Ok(()) };
            match pick(&typed) {
                Pick::Number(n) => self.bug(n as i64)?,
                Pick::Letter('n') => self.file_bug()?,
                Pick::Letter('f') => {
                    show = show.next();
                    page = 0;
                }
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

    /// Filing a bug: its product, title, severity and what happens.
    fn file_bug(&mut self) -> io::Result<()> {
        self.doing("Filing a bug");
        ui::heading(self.call, "File a bug")?;
        let products = self.db.query("SELECT id, name, description FROM products WHERE NOT archived ORDER BY name", &[]).db()?;
        for (i, p) in products.iter().enumerate() {
            self.call.say(&format!(" {CYAN}{:>2}{PLAIN} {BOLD}{}{PLAIN} {DIM}{}{PLAIN}\n", i + 1, ui::pad(&p.get::<_, String>(1), 24), ui::cut(&p.get::<_, String>(2), 40)))?;
        }
        let product: i64 = loop {
            let Some(typed) = ui::answer(self.call, " Which product (its number)? ", 3)? else { return Ok(()) };
            if let Pick::Number(n) = pick(&typed) {
                if let Some(p) = products.get(n.wrapping_sub(1)) {
                    break p.get(0);
                }
            }
        };
        let title = loop {
            let Some(t) = ui::answer(self.call, " In a line, what's wrong? ", 100)? else { return Ok(()) };
            if t.chars().count() >= 5 {
                break t;
            }
            ui::oops(self.call, "A little more than that, please.")?;
        };
        let sev = self.choose(" How bad is it? ", &SEVERITIES, "normal")?;
        let Some(sev) = sev else { return Ok(()) };
        let me = self.me().clone();
        // As the bug's page shows it: who filed it, then what happens.
        let seen = |call: &Call, text: &str| {
            let (cols, _) = ui::window(call);
            let mut lines = vec![format!("{DIM}Filed{PLAIN}    now by {}", printable(&me.handle)), String::new()];
            lines.extend(ui::wrap(text, cols.saturating_sub(4)));
            Preview::Lines(lines)
        };
        let hint = "What happens: what you did, what you expected, and what happened instead.";
        let Some(description) = ui::editor(self.call, &format!("Bug: {title}"), hint, "", Some(&seen))? else { return Ok(()) };
        let id: i64 = self
            .db
            .query_one("INSERT INTO bugs (product_id, title, description, severity, reporter_id) VALUES ($1, $2, $3, $4, $5) RETURNING id", &[&product, &title, &description, &sev, &me.id])
            .db()?
            .get(0);
        self.audit(None, "bug filed", &format!("#{id} {title}"))?;
        ui::done(self.call, &format!("Filed as bug #{id}. Thank you."))?;
        self.pause()?;
        Ok(())
    }

    /// One of `choices` by its number, `current` where they just press
    /// Enter: `None` if they give up.
    pub(crate) fn choose(&mut self, prompt: &str, choices: &[&str], current: &str) -> io::Result<Option<String>> {
        let list: Vec<String> = choices.iter().enumerate().map(|(i, c)| format!("{CYAN}{}{PLAIN} {c}", i + 1)).collect();
        self.call.say(&format!(" {}\n", list.join("   ")))?;
        loop {
            let Some(typed) = ui::answer(self.call, &format!("{prompt}({current}) "), 3)? else { return Ok(None) };
            if typed.is_empty() {
                return Ok(Some(current.to_string()));
            }
            if let Pick::Number(n) = pick(&typed) {
                if let Some(c) = choices.get(n.wrapping_sub(1)) {
                    return Ok(Some(c.to_string()));
                }
            }
        }
    }

    /// Bug `id`, and what can be done with it.
    fn bug(&mut self, id: i64) -> io::Result<()> {
        loop {
            self.doing("Reading a bug");
            let me = self.me().clone();
            let Some(b) = self
                .db
                .query_opt(
                    "SELECT b.title, b.description, b.severity, b.status, p.name, r.handle, a.handle, to_char(b.created_at, 'YYYY-MM-DD HH24:MI'), \
                     to_char(b.updated_at, 'YYYY-MM-DD HH24:MI'), b.reporter_id, r.email \
                     FROM bugs b JOIN products p ON p.id = b.product_id JOIN members r ON r.id = b.reporter_id LEFT JOIN members a ON a.id = b.assignee_id WHERE b.id = $1",
                    &[&id],
                )
                .db()?
            else {
                ui::oops(self.call, &format!("There's no bug #{id}."))?;
                self.pause()?;
                return Ok(());
            };
            let (title, description, sev, status, product, reporter, assignee, filed, updated): (String, String, String, String, String, String, Option<String>, String, String) =
                (b.get(0), b.get(1), b.get(2), b.get(3), b.get(4), b.get(5), b.get(6), b.get(7), b.get(8));
            let (reporter_id, reporter_email): (i64, String) = (b.get(9), b.get(10));
            let (cols, _) = ui::window(self.call);
            let mut lines = vec![
                format!("{DIM}Product{PLAIN}  {}    {DIM}Severity{PLAIN} {}    {DIM}Status{PLAIN} {BOLD}{status}{PLAIN}", printable(&product), severity(&sev)),
                format!("{DIM}Filed{PLAIN}    {filed} by {reporter}    {DIM}Assigned to{PLAIN} {}", assignee.as_deref().unwrap_or("no one")),
                format!("{DIM}Updated{PLAIN}  {updated}"),
                String::new(),
            ];
            lines.extend(ui::wrap(&description, cols.saturating_sub(4)));
            lines.push(String::new());
            let comments = self
                .db
                .query("SELECT m.handle, c.body, c.change, to_char(c.created_at, 'YYYY-MM-DD HH24:MI') FROM bug_comments c JOIN members m ON m.id = c.author_id WHERE c.bug_id = $1 ORDER BY c.id", &[&id])
                .db()?;
            for c in comments {
                let (who, body, change, at): (String, String, String, String) = (c.get(0), c.get(1), c.get(2), c.get(3));
                lines.push(format!("{BOLD}{who}{PLAIN} {DIM}{at}{PLAIN}"));
                if !change.is_empty() {
                    lines.push(format!("{YELLOW}{}{PLAIN}", printable(&change)));
                }
                if !body.is_empty() {
                    lines.extend(ui::wrap(&body, cols.saturating_sub(4)));
                }
                lines.push(String::new());
            }
            if !ui::page(self.call, &format!("Bug #{id}: {}", printable(&title)), &lines)? {
                return Ok(());
            }
            let staff = self.is(Role::Staff);
            let prompt = if staff {
                " C to comment, S status, V severity, A assign, T retitle, Enter to read again, Q to go back: "
            } else {
                " C to comment, Enter to read again, Q to go back: "
            };
            let Some(typed) = ui::answer(self.call, prompt, 3)? else { return Ok(()) };
            let change = match pick(&typed) {
                Pick::Letter('q') => return Ok(()),
                Pick::Letter('c') => {
                    let seen = |call: &Call, text: &str| ui::as_posted(call, &me.handle, text);
                    if let Some(body) = ui::editor(self.call, &format!("A comment on bug #{id}"), "", "", Some(&seen))? {
                        self.db.execute("INSERT INTO bug_comments (bug_id, author_id, body) VALUES ($1, $2, $3)", &[&id, &me.id, &body]).db()?;
                        self.db.execute("UPDATE bugs SET updated_at = now() WHERE id = $1", &[&id]).db()?;
                    }
                    None
                }
                Pick::Letter('s') if staff => self.choose(" Its status? ", &STATUSES, &status)?.filter(|s| *s != status).map(|s| ("status", status.clone(), s)),
                Pick::Letter('v') if staff => self.choose(" Its severity? ", &SEVERITIES, &sev)?.filter(|s| *s != sev).map(|s| ("severity", sev.clone(), s)),
                Pick::Letter('t') if staff => ui::answer(self.call, " Its new title: ", 100)?.filter(|t| t.chars().count() >= 5 && *t != title).map(|t| ("title", title.clone(), t)),
                Pick::Letter('a') if staff => {
                    let typed = ui::answer(self.call, " Assign to which staff handle (- for no one)? ", 20)?;
                    match typed.as_deref() {
                        None | Some("") => None,
                        Some("-") => Some(("assignee", assignee.clone().unwrap_or("no one".into()), "no one".into())),
                        Some(h) => match self.db.query_opt("SELECT handle FROM members WHERE handle_key = lower($1) AND role IN ('staff', 'sysop') AND status = 'active'", &[&h]).db()? {
                            Some(r) => Some(("assignee", assignee.clone().unwrap_or("no one".into()), r.get(0))),
                            None => {
                                ui::oops(self.call, "No staff member has that handle.")?;
                                self.pause()?;
                                None
                            }
                        },
                    }
                }
                _ => None,
            };
            if let Some((what, from, to)) = change {
                self.change_bug(id, what, &from, &to)?;
                if what == "status" && reporter_id != me.id {
                    let name = self.bbs.settings.name.clone();
                    self.mail(&reporter_email, &format!("{name}: bug #{id} is {to}"), &format!("Bug #{id}, \"{title}\", which you filed, is now {to} ({} changed it).\n\nCall the board to read it.", me.handle));
                }
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// Makes a staff change to bug `id`, and writes it on the bug.
    fn change_bug(&mut self, id: i64, what: &str, from: &str, to: &str) -> io::Result<()> {
        let me = self.me().clone();
        let mut t = self.db.transaction().db()?;
        match what {
            "status" => t.execute("UPDATE bugs SET status = $2, updated_at = now() WHERE id = $1", &[&id, &to]),
            "severity" => t.execute("UPDATE bugs SET severity = $2, updated_at = now() WHERE id = $1", &[&id, &to]),
            "title" => t.execute("UPDATE bugs SET title = $2, updated_at = now() WHERE id = $1", &[&id, &to]),
            _ => t.execute("UPDATE bugs SET assignee_id = (SELECT id FROM members WHERE handle = $2), updated_at = now() WHERE id = $1", &[&id, &to]),
        }
        .db()?;
        let change = format!("{what}: {from} → {to}");
        t.execute("INSERT INTO bug_comments (bug_id, author_id, change) VALUES ($1, $2, $3)", &[&id, &me.id, &change]).db()?;
        t.commit().db()?;
        self.audit(None, "bug changed", &format!("#{id} {change}"))?;
        Ok(())
    }
}
