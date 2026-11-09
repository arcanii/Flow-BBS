//! Members' accounts: applying, proving an email address, setting up an
//! authenticator, logging in, a forgotten password, and a member's own
//! profile and security.
//!
//! What a caller is told never says which part was wrong or whether a
//! handle or address is a member's: a login that fails says the same
//! whether the handle, the password or the code was wrong (or the handle
//! is waiting out its lock), and an address already taken goes through the
//! same steps as any other, its owner told by mail and the code that never
//! came never right. Codes and limits are crypto.rs's and United One's.

use std::io;

use flow_bbs::printable;
use postgres::Row;

use crate::crypto::{self, check_code, step_now};
use crate::db::{self, Db, Role};
use crate::session::{Me, Session};
use crate::ui::{self, BOLD, DIM, PLAIN, YELLOW};

/// How long a mailed code is good for, how many tries it gets, and how many
/// may be mailed to one address in an hour.
const CODE_MINUTES: i32 = 10;
const CODE_TRIES: i32 = 5;
const SENDS_PER_HOUR: i64 = 3;
/// Failed logins in a row before a handle waits, and how long it waits.
const LOCK_AFTER: i32 = 5;
const LOCK_MINUTES: i32 = 15;
/// Applications waiting at once (unverified or pending) before the board
/// takes no more, so it can't be made to mail without end.
const OPEN_APPLICATIONS: i64 = 200;
/// Recovery codes a member has at once.
const RECOVERY_CODES: usize = 8;

/// Handles no one may take.
const RESERVED: &[&str] = &["sysop", "admin", "administrator", "root", "guest", "staff", "anie", "moderator", "support", "system", "new", "everyone", "nobody"];

/// Whether `handle` may be one: 3 to 20 letters, digits, `-` or `_`, a
/// letter first, and not one of the reserved.
pub fn handle_ok(handle: &str) -> Result<(), &'static str> {
    let n = handle.chars().count();
    if !(3..=20).contains(&n) {
        return Err("A handle is 3 to 20 letters long.");
    }
    if !handle.starts_with(|c: char| c.is_ascii_alphabetic()) || !handle.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("A handle starts with a letter and has only letters, digits, - and _.");
    }
    if RESERVED.contains(&handle.to_ascii_lowercase().as_str()) {
        return Err("That handle is kept for the board.");
    }
    Ok(())
}

/// Whether `email` is an address mail can go to.
pub fn email_ok(email: &str) -> bool {
    email.len() <= 254 && !email.contains(char::is_whitespace) && email.parse::<lettre::Address>().is_ok_and(|a| a.domain().contains('.'))
}

/// Whether `password` will do for `handle`.
pub fn password_ok(password: &str, handle: &str) -> Result<(), &'static str> {
    let n = password.chars().count();
    if n < 10 {
        return Err("A password is at least 10 letters long.");
    }
    if n > 128 {
        return Err("A password is at most 128 letters long.");
    }
    if password.eq_ignore_ascii_case(handle) || password.chars().all(|c| password.starts_with(c)) {
        return Err("That password is too easy to guess.");
    }
    Ok(())
}

/// What's mailed with a code's row.
enum Send<'a> {
    /// The code itself.
    Code,
    /// No code: a notice instead (the address belongs to another member).
    Notice(&'a str),
    /// Nothing: there's no member to mail.
    Nothing,
}

/// How a mailed code came back.
enum Taken {
    Right,
    Wrong,
    /// Expired, used up, or never sent: a new one is needed.
    Spent,
}

/// A member's row, as logging in reads it.
struct Account {
    id: i64,
    handle: String,
    password_hash: String,
    seed: Option<Vec<u8>>,
    last_step: Option<i64>,
    role: Role,
    status: String,
    note: String,
    email: String,
    email_verified: bool,
    locked: bool,
}

const ACCOUNT: &str = "SELECT id, handle, password_hash, seed_sealed, last_step, role, status, status_note, email, email_verified, coalesce(locked_until > now(), false) FROM members";

impl Account {
    fn from(r: &Row) -> Account {
        Account {
            id: r.get(0),
            handle: r.get(1),
            password_hash: r.get(2),
            seed: r.get(3),
            last_step: r.get(4),
            role: Role::parse(r.get(5)).unwrap_or(Role::Member),
            status: r.get(6),
            note: r.get(7),
            email: r.get(8),
            email_verified: r.get(9),
            locked: r.get(10),
        }
    }
}

/// How a second factor came: a code's step, or a recovery code's hash.
enum Factor {
    Step(u64),
    Recovery(String),
}

impl Session<'_> {
    /// Applying for membership, start to end.
    pub fn apply(&mut self) -> io::Result<()> {
        self.doing("Applying");
        ui::heading(self.call, "Apply for membership")?;
        self.db.execute("DELETE FROM members WHERE status = 'unverified' AND created_at < now() - interval '1 day'", &[]).db()?;
        let ip = self.ip();
        if self.bbs.guard.lock().unwrap().applications(ip) >= crate::APPLICATIONS_PER_ADDRESS {
            ui::oops(self.call, "There have been enough applications from where you're calling for now. Please try again in an hour.")?;
            self.pause()?;
            return Ok(());
        }
        let open: i64 = self.db.query_one("SELECT count(*) FROM members WHERE status IN ('unverified', 'pending')", &[]).db()?.get(0);
        if open >= OPEN_APPLICATIONS {
            ui::oops(self.call, "The board isn't taking applications just now. Please try again another day.")?;
            self.pause()?;
            return Ok(());
        }
        let name = self.bbs.settings.name.clone();
        self.call.show(&self.bbs.screens.rules, &[("board", name.as_str())])?;
        if ui::yes(self.call, "\n Do you agree to the board's rules?")? != Some(true) {
            return Ok(());
        }
        ui::heading(self.call, "Apply for membership")?;
        self.call.say(&format!(" {DIM}Esc at any question gives up the application.{PLAIN}\n\n"))?;
        let handle = loop {
            let Some(h) = ui::answer(self.call, " Handle (what the board calls you): ", 20)? else { return Ok(()) };
            if let Err(why) = handle_ok(&h) {
                ui::oops(self.call, why)?;
            } else if self.db.query_opt("SELECT 1 FROM members WHERE handle_key = lower($1)", &[&h]).db()?.is_some() {
                ui::oops(self.call, "That handle is taken.")?;
            } else {
                break h;
            }
        };
        let real_name = loop {
            let Some(n) = ui::answer(self.call, " Your name: ", 60)? else { return Ok(()) };
            if !n.is_empty() {
                break n;
            }
        };
        let Some(email) = self.ask_email(" Your email address: ")? else { return Ok(()) };
        let Some(password) = self.ask_new_password(&handle)? else { return Ok(()) };
        self.bbs.guard.lock().unwrap().applied(ip);

        let taken = self.db.query_opt("SELECT 1 FROM members WHERE email_key = lower($1)", &[&email]).db()?.is_some();
        let id = if taken {
            None
        } else {
            let hash = crypto::hash_password(&password);
            let added = self.db.query_one(
                "INSERT INTO members (handle, handle_key, real_name, email, email_key, password_hash, applied_from) VALUES ($1, lower($1), $2, $3, lower($3), $4, $5) RETURNING id",
                &[&handle, &real_name, &email, &hash, &ip.to_string()],
            );
            match added {
                Ok(r) => Some(r.get::<_, i64>(0)),
                Err(e) if db::taken(&e) => {
                    ui::oops(self.call, "That handle or address was taken a moment ago. Please start again.")?;
                    self.pause()?;
                    return Ok(());
                }
                Err(e) => return Err(db::broke(e)),
            }
        };
        self.audit(id, "applied", &handle)?;
        let notice = format!(
            "Someone just applied to {name} with this address. It's already a member's, so nothing more will happen.\n\n\
             If it was you: you're already a member. Log in, or choose \"Forgot your password\".\nIf it wasn't you, there's nothing to do."
        );
        if !self.prove_email(id, &email, "verify", if id.is_some() { Send::Code } else { Send::Notice(&notice) })? {
            if !self.call.hung_up() {
                self.call.say(&format!("\n You can finish within a day: log in with {BOLD}{}{PLAIN} and your password.\n", printable(&handle)))?;
                self.pause()?;
            }
            return Ok(());
        }
        let id = id.expect("only a member's own code is right");
        self.db.execute("UPDATE members SET email_verified = true WHERE id = $1", &[&id]).db()?;
        self.finish_application(id, &handle)
    }

    /// The rest of an application once the address is proved: the
    /// authenticator, then it's the sysop's.
    fn finish_application(&mut self, id: i64, handle: &str) -> io::Result<()> {
        if !self.enrol(id, handle)? {
            if !self.call.hung_up() {
                self.call.say(&format!("\n You can finish within a day: log in with {BOLD}{}{PLAIN} and your password.\n", printable(handle)))?;
                self.pause()?;
            }
            return Ok(());
        }
        self.db.execute("UPDATE members SET status = 'pending' WHERE id = $1 AND status = 'unverified'", &[&id]).db()?;
        self.audit(Some(id), "application complete", handle)?;
        let name = self.bbs.settings.name.clone();
        let real: String = self.db.query_one("SELECT real_name FROM members WHERE id = $1", &[&id]).db()?.get(0);
        self.tell_sysops(&format!("{name}: {handle} has applied"), &format!("{handle} ({real}) has applied to {name}.\n\nCall the board and choose Sysop, then Applications, to decide."))?;
        ui::heading(self.call, "Thank you")?;
        self.call.say(" Your application is with the sysop. You'll hear by email when it's been decided.\n")?;
        self.pause()?;
        Ok(())
    }

    /// Asks for an address until one will do.
    fn ask_email(&mut self, prompt: &str) -> io::Result<Option<String>> {
        loop {
            let Some(e) = ui::answer(self.call, prompt, 254)? else { return Ok(None) };
            if email_ok(&e) {
                return Ok(Some(e));
            }
            ui::oops(self.call, "That isn't an address mail can go to.")?;
        }
    }

    /// Asks for a new password twice, until one will do.
    fn ask_new_password(&mut self, handle: &str) -> io::Result<Option<String>> {
        loop {
            let Some(p) = self.call.ask_secret(" New password (10 letters or more): ", 128)? else { return Ok(None) };
            if let Err(why) = password_ok(&p, handle) {
                ui::oops(self.call, why)?;
                continue;
            }
            let Some(again) = self.call.ask_secret(" The same again: ", 128)? else { return Ok(None) };
            if crypto::same(p.as_bytes(), again.as_bytes()) {
                return Ok(Some(p));
            }
            ui::oops(self.call, "Those weren't the same.")?;
        }
    }

    /// Mails a code for `purpose` to `email` (or `send` says what else) and
    /// asks for it until it's right: `false` if it never is. Where there's no
    /// member (`id` is `None`), a code no one knows is kept instead, so the
    /// steps are the same and the code is never right.
    fn prove_email(&mut self, id: Option<i64>, email: &str, purpose: &str, send: Send) -> io::Result<bool> {
        let mut sent = self.mail_code(id, email, purpose, &send)?;
        loop {
            if sent {
                self.call.say(&format!(" {DIM}A code is on its way to {}. It's good for 10 minutes.{PLAIN}\n", printable(email)))?;
            } else {
                ui::oops(self.call, "No code could be sent just now (too many in the last hour, or the mail is down). R tries again.")?;
            }
            let Some(typed) = ui::answer(self.call, " The code from the mail (R sends another): ", 10)? else { return Ok(false) };
            if typed.eq_ignore_ascii_case("r") {
                sent = self.mail_code(id, email, purpose, &send)?;
                continue;
            }
            match self.take_code(id, email, purpose, &typed)? {
                Taken::Right => return Ok(true),
                Taken::Wrong => ui::oops(self.call, "That code isn't right.")?,
                Taken::Spent => ui::oops(self.call, "That code has run out (10 minutes, or 5 tries). R sends a new one.")?,
            }
            sent = true;
        }
    }

    /// Keeps a new code for `email` and mails it: `false` if it can't go
    /// (three in the last hour already, or the mail failed).
    fn mail_code(&mut self, id: Option<i64>, email: &str, purpose: &str, send: &Send) -> io::Result<bool> {
        self.db.execute("DELETE FROM email_codes WHERE created_at < now() - interval '1 day'", &[]).db()?;
        // Counted for this member alone (or for no member), so codes kept for
        // someone who typed a member's address with the wrong handle don't use
        // up the member's own.
        let recent: i64 = self
            .db
            .query_one("SELECT count(*) FROM email_codes WHERE email_key = lower($1) AND member_id IS NOT DISTINCT FROM $2 AND created_at > now() - interval '1 hour'", &[&email, &id])
            .db()?
            .get(0);
        if recent >= SENDS_PER_HOUR {
            return Ok(false);
        }
        let code = crypto::digits(6);
        let hash = self.bbs.settings.keys.hash(purpose, &code);
        let mut t = self.db.transaction().db()?;
        t.execute("UPDATE email_codes SET consumed = true WHERE email_key = lower($1) AND purpose = $2 AND NOT consumed", &[&email, &purpose]).db()?;
        t.execute(
            "INSERT INTO email_codes (member_id, email_key, purpose, code_hash, expires_at) VALUES ($1, lower($2), $3, $4, now() + make_interval(mins => $5))",
            &[&id, &email, &purpose, &hash, &CODE_MINUTES],
        )
        .db()?;
        t.commit().db()?;
        let name = &self.bbs.settings.name;
        Ok(match send {
            Send::Code => self.mail(
                email,
                &format!("{name}: your code is {code}"),
                &format!("Your code for {name} is:\n\n    {code}\n\nIt's good for 10 minutes. If you didn't ask for it, someone typed your address: there's nothing to do, and no one gets in with it."),
            ),
            Send::Notice(text) => self.mail(email, &format!("{name}: someone used your address"), text),
            Send::Nothing => true,
        })
    }

    /// Checks `typed` against the last code for `email` and `purpose`, a try
    /// counted, the code used up when it's right.
    fn take_code(&mut self, id: Option<i64>, email: &str, purpose: &str, typed: &str) -> io::Result<Taken> {
        let row = self
            .db
            .query_opt(
                "SELECT id, code_hash FROM email_codes WHERE email_key = lower($1) AND purpose = $2 AND member_id IS NOT DISTINCT FROM $3 AND NOT consumed AND expires_at > now() ORDER BY id DESC LIMIT 1",
                &[&email, &purpose, &id],
            )
            .db()?;
        let Some(row) = row else { return Ok(Taken::Spent) };
        let (code_id, hash): (i64, String) = (row.get(0), row.get(1));
        let tries: i32 = self.db.query_one("UPDATE email_codes SET attempts = attempts + 1 WHERE id = $1 RETURNING attempts", &[&code_id]).db()?.get(0);
        if tries > CODE_TRIES {
            self.db.execute("UPDATE email_codes SET consumed = true WHERE id = $1", &[&code_id]).db()?;
            return Ok(Taken::Spent);
        }
        let typed: String = typed.chars().filter(|c| c.is_ascii_digit()).collect();
        if id.is_none() || !crypto::same(self.bbs.settings.keys.hash(purpose, &typed).as_bytes(), hash.as_bytes()) {
            return Ok(Taken::Wrong);
        }
        // Used up here, and only if it wasn't already: a code is good once.
        let used = self.db.execute("UPDATE email_codes SET consumed = true WHERE id = $1 AND NOT consumed", &[&code_id]).db()?;
        Ok(if used == 1 { Taken::Right } else { Taken::Spent })
    }

    /// Sets up an authenticator for member `id`: a new seed shown as a QR
    /// code and as text, kept only once a code from it comes back right,
    /// then new recovery codes. `false` if it never comes right.
    pub fn enrol(&mut self, id: i64, handle: &str) -> io::Result<bool> {
        let seed = crypto::new_seed();
        let uri = crypto::seed_uri(&seed, &self.bbs.settings.name, handle);
        ui::heading(self.call, "Set up your authenticator")?;
        self.call.say(" Scan this with an authenticator app on your phone: Google Authenticator,\n Microsoft Authenticator, 1Password, Authy, or any other.\n")?;
        for line in ui::qr(&uri) {
            self.call.say(&format!(" {line}\n"))?;
        }
        let text = crypto::seed_text(&seed);
        let grouped: Vec<String> = text.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        self.call.say(&format!(" {DIM}Or type it in: {}{PLAIN}\n", grouped.join(" ")))?;
        for _ in 0..CODE_TRIES {
            let Some(typed) = ui::answer(self.call, " The 6-digit code your app shows: ", 8)? else { return Ok(false) };
            if let Some(step) = check_code(&seed, &typed.replace(' ', ""), step_now(), None) {
                let sealed = self.bbs.settings.keys.seal(id, &seed);
                self.db.execute("UPDATE members SET seed_sealed = $2, last_step = $3 WHERE id = $1", &[&id, &sealed, &(step as i64)]).db()?;
                self.audit(Some(id), "authenticator set up", handle)?;
                self.new_recovery_codes(id)?;
                return Ok(true);
            }
            ui::oops(self.call, "That isn't the code. Is the phone's clock set automatically?")?;
        }
        Ok(false)
    }

    /// New recovery codes for member `id`, the old ones gone, shown once.
    fn new_recovery_codes(&mut self, id: i64) -> io::Result<()> {
        let codes: Vec<String> = (0..RECOVERY_CODES).map(|_| crypto::recovery_code()).collect();
        let mut t = self.db.transaction().db()?;
        t.execute("DELETE FROM recovery_codes WHERE member_id = $1", &[&id]).db()?;
        for c in &codes {
            let hash = self.bbs.settings.keys.hash("recovery", &crypto::plain_recovery_code(c));
            t.execute("INSERT INTO recovery_codes (member_id, code_hash) VALUES ($1, $2)", &[&id, &hash]).db()?;
        }
        t.commit().db()?;
        ui::heading(self.call, "Your recovery codes")?;
        self.call.say(" If your phone is lost, each of these gets you in once, in place of the\n app's code. Write them down and keep them somewhere safe: they won't be\n shown again.\n\n")?;
        for pair in codes.chunks(2) {
            self.call.say(&format!("     {YELLOW}{}{PLAIN}\n", pair.join("     ")))?;
        }
        self.call.say("\n")?;
        self.pause()?;
        Ok(())
    }

    /// Checks a second factor typed for `a`: the app's code (past the last
    /// step taken) or a recovery code not yet used. Nothing is used up yet.
    fn factor(&mut self, a: &Account, typed: &str) -> io::Result<Option<Factor>> {
        let Some(sealed) = &a.seed else { return Ok(None) };
        let Some(seed) = self.bbs.settings.keys.open(a.id, sealed) else {
            eprintln!("anie-bbs: member {}'s authenticator seed won't open: is ANIE_BBS_SECRET_KEY the one it was sealed with?", a.id);
            return Ok(None);
        };
        if let Some(step) = check_code(&seed, &typed.replace(' ', ""), step_now(), a.last_step.map(|s| s as u64)) {
            return Ok(Some(Factor::Step(step)));
        }
        let plain = crypto::plain_recovery_code(typed);
        if plain.len() == 10 {
            let hash = self.bbs.settings.keys.hash("recovery", &plain);
            if self.db.query_opt("SELECT 1 FROM recovery_codes WHERE member_id = $1 AND code_hash = $2 AND used_at IS NULL", &[&a.id, &hash]).db()?.is_some() {
                return Ok(Some(Factor::Recovery(hash)));
            }
        }
        Ok(None)
    }

    /// Uses up a second factor: `false` if it was used meanwhile (by a call
    /// that got there first with the same code).
    fn spend(&mut self, id: i64, f: &Factor) -> io::Result<bool> {
        let n = match f {
            Factor::Step(step) => self.db.execute("UPDATE members SET last_step = $2 WHERE id = $1 AND (last_step IS NULL OR last_step < $2)", &[&id, &(*step as i64)]).db()?,
            Factor::Recovery(hash) => self.db.execute("UPDATE recovery_codes SET used_at = now() WHERE member_id = $1 AND code_hash = $2 AND used_at IS NULL", &[&id, hash]).db()?,
        };
        Ok(n == 1)
    }

    /// Logging in: `true` once they're in as an active member.
    pub fn login(&mut self) -> io::Result<bool> {
        self.doing("Logging in");
        ui::heading(self.call, "Log in")?;
        let ip = self.ip();
        if self.bbs.guard.lock().unwrap().failures(ip) >= crate::FAILURES_PER_ADDRESS {
            ui::oops(self.call, "There have been too many tries from where you're calling. Please wait a while and call again.")?;
            self.pause()?;
            return Ok(false);
        }
        let Some(handle) = ui::answer(self.call, " Handle: ", 20)? else { return Ok(false) };
        let Some(password) = self.call.ask_secret(" Password: ", 128)? else { return Ok(false) };
        let Some(code) = self.call.ask_secret(" Code from your authenticator (or a recovery code): ", 16)? else { return Ok(false) };

        let account = self.db.query_opt(&format!("{ACCOUNT} WHERE handle_key = lower($1)"), &[&handle]).db()?.map(|r| Account::from(&r));
        let password_right = crypto::check_password(&password, account.as_ref().map(|a| a.password_hash.as_str()));
        let factor = match &account {
            Some(a) => self.factor(a, &code)?,
            None => None,
        };
        // No authenticator is no second factor to check: an application not
        // yet finished, or one the sysop reset (mailed codes stand in below).
        let in_ = match &account {
            Some(a) if password_right && !a.locked && (factor.is_some() || a.seed.is_none()) => match &factor {
                Some(f) => self.spend(a.id, f)?,
                None => true,
            },
            _ => false,
        };
        let Some(a) = account.filter(|_| in_) else {
            self.bbs.guard.lock().unwrap().failed(ip);
            let id = self.db.query_opt("SELECT id FROM members WHERE handle_key = lower($1)", &[&handle]).db()?.map(|r| r.get::<_, i64>(0));
            if let Some(id) = id {
                self.db
                    .execute(
                        "UPDATE members SET failed_logins = CASE WHEN failed_logins + 1 >= $2 THEN 0 ELSE failed_logins + 1 END, \
                         locked_until = CASE WHEN failed_logins + 1 >= $2 THEN now() + make_interval(mins => $3) ELSE locked_until END WHERE id = $1",
                        &[&id, &LOCK_AFTER, &LOCK_MINUTES],
                    )
                    .db()?;
            }
            self.audit(id, "login failed", &handle)?;
            ui::oops(self.call, "That handle, password or code isn't right.")?;
            self.call.say(&format!(" {DIM}After several tries in a row, a handle waits 15 minutes before it can log in again.{PLAIN}\n"))?;
            self.pause()?;
            return Ok(false);
        };
        self.db.execute("UPDATE members SET failed_logins = 0, locked_until = NULL WHERE id = $1", &[&a.id]).db()?;

        match a.status.as_str() {
            "unverified" => {
                self.call.say("\n Let's finish your application.\n")?;
                if !a.email_verified {
                    if !self.prove_email(Some(a.id), &a.email, "verify", Send::Code)? {
                        return Ok(false);
                    }
                    self.db.execute("UPDATE members SET email_verified = true WHERE id = $1", &[&a.id]).db()?;
                }
                self.finish_application(a.id, &a.handle)?;
                return Ok(false);
            }
            "pending" => {
                self.call.say("\n Your application is still with the sysop. You'll hear by email when it's been decided.\n")?;
                self.pause()?;
                return Ok(false);
            }
            "rejected" | "suspended" => {
                let what = if a.status == "rejected" { "Your application wasn't accepted" } else { "Your membership is suspended" };
                let why = if a.note.is_empty() { String::new() } else { format!(": {}", printable(&a.note)) };
                self.call.say(&format!("\n {what}{why}.\n"))?;
                self.pause()?;
                return Ok(false);
            }
            _ => {}
        }
        if a.seed.is_none() {
            // The sysop reset their authenticator: the address proves it's
            // them before a new one is set up.
            self.call.say("\n Your authenticator was reset, so a code is coming to your email first.\n")?;
            if !self.prove_email(Some(a.id), &a.email, "enrol", Send::Code)? || !self.enrol(a.id, &a.handle)? {
                return Ok(false);
            }
        }
        self.me = Some(Me { id: a.id, handle: a.handle.clone(), role: a.role });
        self.doing("Logging in");
        self.db.execute("UPDATE members SET last_login_at = now() WHERE id = $1", &[&a.id]).db()?;
        self.audit(None, "login", &a.handle)?;
        if let Some(Factor::Recovery(_)) = factor {
            let left: i64 = self.db.query_one("SELECT count(*) FROM recovery_codes WHERE member_id = $1 AND used_at IS NULL", &[&a.id]).db()?.get(0);
            self.call.say(&format!("\n {YELLOW}You used a recovery code; {left} are left.{PLAIN} Profile and security sets up your\n authenticator again and makes new ones.\n"))?;
            self.pause()?;
        }
        Ok(true)
    }

    /// A forgotten password: a code to the member's address, then a new one.
    /// The steps are the same whether or not the handle and address are a
    /// member's.
    pub fn forgot(&mut self) -> io::Result<()> {
        self.doing("Logging in");
        ui::heading(self.call, "Forgot your password")?;
        self.call.say(" Your handle and the address you applied with, and a code will come to it.\n\n")?;
        let Some(handle) = ui::answer(self.call, " Handle: ", 20)? else { return Ok(()) };
        let Some(email) = self.ask_email(" Email address: ")? else { return Ok(()) };
        let found = self
            .db
            .query_opt("SELECT id, handle FROM members WHERE handle_key = lower($1) AND email_key = lower($2) AND email_verified AND status IN ('active', 'pending')", &[&handle, &email])
            .db()?;
        let (id, handle) = match found {
            Some(r) => (Some(r.get::<_, i64>(0)), r.get::<_, String>(1)),
            None => (None, handle),
        };
        let send = if id.is_some() { Send::Code } else { Send::Nothing };
        if !self.prove_email(id, &email, "reset", send)? {
            return Ok(());
        }
        let id = id.expect("only a member's own code is right");
        let Some(password) = self.ask_new_password(&handle)? else { return Ok(()) };
        let hash = crypto::hash_password(&password);
        self.db.execute("UPDATE members SET password_hash = $2, failed_logins = 0, locked_until = NULL WHERE id = $1", &[&id, &hash]).db()?;
        self.audit(Some(id), "password reset", &handle)?;
        let name = self.bbs.settings.name.clone();
        self.mail(&email, &format!("{name}: your password was changed"), &format!("The password for {handle} on {name} was just changed, with a code mailed here.\n\nIf that wasn't you, tell the sysop."));
        ui::done(self.call, "Your password is changed. Log in with it and your authenticator's code.")?;
        self.pause()?;
        Ok(())
    }

    /// A member's own profile and security.
    pub fn profile(&mut self) -> io::Result<()> {
        loop {
            self.doing("In their profile");
            let me = self.me().clone();
            let r = self
                .db
                .query_one(
                    "SELECT real_name, email, to_char(created_at, 'YYYY-MM-DD'), (SELECT count(*) FROM recovery_codes WHERE member_id = $1 AND used_at IS NULL) FROM members WHERE id = $1",
                    &[&me.id],
                )
                .db()?;
            let (real, email, since, left): (String, String, String, i64) = (r.get(0), r.get(1), r.get(2), r.get(3));
            ui::heading(self.call, "Profile and security")?;
            self.call.say(&format!(
                " Handle      {BOLD}{}{PLAIN}\n Name        {}\n Email       {}\n Role        {}\n Member since {since}\n Recovery codes left: {left}\n\n",
                me.handle,
                printable(&real),
                printable(&email),
                me.role.as_str()
            ))?;
            self.call.say(&format!(
                "  {0}[p]{PLAIN} Change your password\n  {0}[a]{PLAIN} Move your authenticator to a new phone\n  {0}[r]{PLAIN} New recovery codes\n  {0}[e]{PLAIN} Change your email address\n  {0}[b]{PLAIN} Back\n\n",
                ui::CYAN
            ))?;
            let Some(choice) = ui::answer(self.call, "  Your choice: ", 1)? else { return Ok(()) };
            match choice.to_ascii_lowercase().as_str() {
                "p" => self.change_password()?,
                "a" => {
                    if self.confirm_factor()? && self.enrol(me.id, &me.handle)? {
                        ui::done(self.call, "Your new authenticator is set up; the old one's codes work no more.")?;
                        self.pause()?;
                    }
                }
                "r" => {
                    if self.confirm_factor()? {
                        self.new_recovery_codes(me.id)?;
                    }
                }
                "e" => self.change_email()?,
                "b" => return Ok(()),
                _ => {}
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// Asks for the authenticator's code (or a recovery code) and uses it
    /// up: `true` if it was right.
    fn confirm_factor(&mut self) -> io::Result<bool> {
        let id = self.me().id;
        let Some(typed) = self.call.ask_secret(" Code from your authenticator (or a recovery code): ", 16)? else { return Ok(false) };
        let a = Account::from(&self.db.query_one(&format!("{ACCOUNT} WHERE id = $1"), &[&id]).db()?);
        match self.factor(&a, &typed)? {
            Some(f) if self.spend(id, &f)? => Ok(true),
            _ => {
                ui::oops(self.call, "That code isn't right.")?;
                self.pause()?;
                Ok(false)
            }
        }
    }

    fn change_password(&mut self) -> io::Result<()> {
        let me = self.me().clone();
        let Some(current) = self.call.ask_secret(" Your password now: ", 128)? else { return Ok(()) };
        let r = self.db.query_one("SELECT password_hash, email FROM members WHERE id = $1", &[&me.id]).db()?;
        let (hash, email): (String, String) = (r.get(0), r.get(1));
        if !crypto::check_password(&current, Some(&hash)) {
            ui::oops(self.call, "That isn't your password.")?;
            self.pause()?;
            return Ok(());
        }
        let Some(new) = self.ask_new_password(&me.handle)? else { return Ok(()) };
        self.db.execute("UPDATE members SET password_hash = $2 WHERE id = $1", &[&me.id, &crypto::hash_password(&new)]).db()?;
        self.audit(None, "password changed", &me.handle)?;
        let name = self.bbs.settings.name.clone();
        self.mail(&email, &format!("{name}: your password was changed"), &format!("The password for {} on {name} was just changed.\n\nIf that wasn't you, tell the sysop.", me.handle));
        ui::done(self.call, "Your password is changed.")?;
        self.pause()?;
        Ok(())
    }

    fn change_email(&mut self) -> io::Result<()> {
        let me = self.me().clone();
        let Some(current) = self.call.ask_secret(" Your password: ", 128)? else { return Ok(()) };
        let r = self.db.query_one("SELECT password_hash, email FROM members WHERE id = $1", &[&me.id]).db()?;
        let (hash, old): (String, String) = (r.get(0), r.get(1));
        if !crypto::check_password(&current, Some(&hash)) {
            ui::oops(self.call, "That isn't your password.")?;
            self.pause()?;
            return Ok(());
        }
        let Some(email) = self.ask_email(" Your new email address: ")? else { return Ok(()) };
        let taken = self.db.query_opt("SELECT 1 FROM members WHERE email_key = lower($1)", &[&email]).db()?.is_some();
        let name = self.bbs.settings.name.clone();
        let notice = format!("Someone tried to change their address on {name} to this one. It's already a member's, so nothing more will happen.");
        let who = if taken { None } else { Some(me.id) };
        if !self.prove_email(who, &email, "change", if taken { Send::Notice(&notice) } else { Send::Code })? {
            return Ok(());
        }
        match self.db.execute("UPDATE members SET email = $2, email_key = lower($2) WHERE id = $1", &[&me.id, &email]) {
            Ok(_) => {}
            Err(e) if db::taken(&e) => {
                ui::oops(self.call, "That address was taken a moment ago.")?;
                self.pause()?;
                return Ok(());
            }
            Err(e) => return Err(db::broke(e)),
        }
        self.audit(None, "email changed", &me.handle)?;
        self.mail(&old, &format!("{name}: your address was changed"), &format!("The address for {} on {name} was just changed from this one.\n\nIf that wasn't you, tell the sysop.", me.handle));
        ui::done(self.call, "Your address is changed.")?;
        self.pause()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_is_three_to_twenty_letters_a_letter_first_and_not_the_boards() {
        assert!(handle_ok("bryan").is_ok());
        assert!(handle_ok("Night_Owl-2").is_ok());
        assert!(handle_ok("ab").is_err());
        assert!(handle_ok("a".repeat(21).as_str()).is_err());
        assert!(handle_ok("2fast").is_err());
        assert!(handle_ok("bad name").is_err());
        assert!(handle_ok("浦和").is_err());
        assert!(handle_ok("SysOp").is_err());
    }

    #[test]
    fn an_address_is_one_mail_can_go_to() {
        assert!(email_ok("bryan@example.com"));
        assert!(!email_ok("bryan@localhost"));
        assert!(!email_ok("bryan example.com"));
        assert!(!email_ok("Bryan <bryan@example.com>"));
        assert!(!email_ok(""));
    }

    #[test]
    fn a_password_is_long_enough_and_not_the_handle() {
        assert!(password_ok("a long enough one", "bryan").is_ok());
        assert!(password_ok("short", "bryan").is_err());
        assert!(password_ok("BryanBryan", "bryanbryan").is_err());
        assert!(password_ok("aaaaaaaaaaaa", "bryan").is_err());
        assert!(password_ok(&"x".repeat(129), "bryan").is_err());
    }
}
