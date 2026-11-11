//! Calls to the Anie Solutions BBS served for real: over SSH, as `dial`
//! reaches it, against a PostgreSQL of the test's own (a database made for
//! each test and dropped after), its mail kept for the test to read.
//!
//! They need a PostgreSQL to make databases in, named by
//! `ANIE_BBS_TEST_DATABASE_URL` (as `postgres://postgres@127.0.0.1:55432/postgres`);
//! without it each test says so and passes, so `cargo test --workspace`
//! runs anywhere.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use anie_bbs::crypto::{self, Keys};
use anie_bbs::mail::Memory;
use anie_bbs::settings::Settings;
use anie_bbs::{db, Bbs};
use flow_bbs::{EDIT_KEYS, MENU_KEYS};
use postgres::{Client, NoTls};
use russh::client;
use russh::keys::PublicKeyOrCertificate;
use russh::ChannelMsg;

/// Runs `f` on a thread of its own: the database's client blocks, and
/// can't from inside the test's async runtime.
fn blocking<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().unwrap())
}

const KEY: &str = "5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";

/// A board of the test's own: where to call it, its database, its mail.
struct Board {
    address: SocketAddr,
    url: String,
    mail: Arc<Memory>,
    name: String,
    base: String,
    /// Where members' games are kept between calls (read by a test of
    /// Unix's).
    #[cfg_attr(not(unix), allow(dead_code))]
    saves: std::path::PathBuf,
}

impl Board {
    /// A board on a database of its own, or `None` where there's no
    /// PostgreSQL to test with.
    fn new(test: &str) -> Option<Board> {
        blocking(|| Board::make(test))
    }

    fn make(test: &str) -> Option<Board> {
        let Ok(base) = std::env::var("ANIE_BBS_TEST_DATABASE_URL") else {
            eprintln!("{test}: ANIE_BBS_TEST_DATABASE_URL isn't set, so no board to call");
            return None;
        };
        let name = format!("anie_test_{}_{test}", std::process::id());
        let mut admin = Client::connect(&base, NoTls).unwrap();
        admin.batch_execute(&format!("DROP DATABASE IF EXISTS {name}")).unwrap();
        admin.batch_execute(&format!("CREATE DATABASE {name}")).unwrap();
        let url = format!("{}/{name}", base.rsplit_once('/').unwrap().0);
        let dir = std::env::temp_dir().join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        let (mines, dungeon) = (stand_in_for_mines(&dir), stand_in_for_dungeon(&dir));
        let env = [
            ("ANIE_BBS_DATABASE_URL", url.clone()),
            ("ANIE_BBS_SECRET_KEY", KEY.into()),
            ("ANIE_BBS_MAIL", "dev".into()),
            ("ANIE_BBS_SCREENS", concat!(env!("CARGO_MANIFEST_DIR"), "/screens").into()),
            ("ANIE_BBS_HOST_KEY", dir.join("host_key").display().to_string()),
            // A door that ends at once, standing in for flow-cat.
            ("ANIE_BBS_NEKO", if cfg!(windows) { "hostname" } else { "true" }.into()),
            ("ANIE_BBS_MINES", mines),
            ("ANIE_BBS_DUNGEON", dungeon),
            ("ANIE_BBS_SAVES", dir.join("saves").display().to_string()),
            // Faces to typeset the bulletin in, where the test's machine has them.
            ("ANIE_BBS_FONTS", std::env::var("ANIE_BBS_TEST_FONTS").unwrap_or_default()),
        ];
        let (settings, _) = Settings::from_lookup(|k| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())).unwrap();
        let mail = Arc::new(Memory::default());
        let bbs = Bbs::new(settings, Box::new(mail.clone())).unwrap();
        bbs.migrate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || anie_bbs::serve(bbs, Some(listener)));
        Some(Board { address, url, mail, name, base, saves: dir.join("saves") })
    }

    /// Does `f` with the board's database.
    fn sql<T: Send>(&self, f: impl FnOnce(&mut Client) -> T + Send) -> T {
        blocking(|| f(&mut Client::connect(&self.url, NoTls).unwrap()))
    }

    /// The last mail to `to`: its subject and body.
    fn last_mail(&self, to: &str) -> (String, String) {
        let sent = self.mail.sent.lock().unwrap();
        let (_, s, b) = sent.iter().rev().find(|(t, _, _)| t == to).unwrap_or_else(|| panic!("no mail to {to}: {sent:?}"));
        (s.clone(), b.clone())
    }

    /// The code in the last mail to `to`.
    fn code_for(&self, to: &str) -> String {
        let (_, body) = self.last_mail(to);
        let at = body.find("is:\n\n    ").unwrap_or_else(|| panic!("no code in {body:?}")) + 9;
        body[at..at + 6].to_string()
    }

    /// A member put straight in the database: active, with `seed` set up.
    fn member(&self, handle: &str, password: &str, seed: &[u8], role: &str) -> i64 {
        self.sql(|db| {
        let id: i64 = db
            .query_one(
                "INSERT INTO members (handle, handle_key, real_name, email, email_key, email_verified, password_hash, role, status) \
                 VALUES ($1, lower($1), $1, $2, $2, true, $3, $4, 'active') RETURNING id",
                &[&handle, &format!("{handle}@example.com"), &crypto::hash_password(password), &role],
            )
            .unwrap()
            .get(0);
        let sealed = Keys::from_hex(KEY).unwrap().seal(id, seed);
        db.execute("UPDATE members SET seed_sealed = $2 WHERE id = $1", &[&id, &sealed]).unwrap();
        id
        })
    }
}

/// A door standing in for flow-mines, in `dir`: it says the best times it
/// was given, then takes a time for Small from the caller's keys and keeps
/// it as a win there would. On Windows, a door that ends at once.
fn stand_in_for_mines(dir: &std::path::Path) -> String {
    if cfg!(windows) {
        return "hostname".into();
    }
    let script = dir.join("mines.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\n\
         kept=\"$XDG_DATA_HOME/flow-mines/best.json\"\n\
         printf 'given: '; cat \"$kept\" 2>/dev/null; printf '\\r\\ntime: '\n\
         read secs\n\
         mkdir -p \"$XDG_DATA_HOME/flow-mines\"\n\
         printf '{\"Small\": %s}' \"$secs\" > \"$kept\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script.display().to_string()
}

/// A stand-in for Dungeon: says where it runs and whether it has the
/// server's variables, takes up a game kept there, and keeps one on
/// SAVE, until QUIT. On Windows, a program that ends.
fn stand_in_for_dungeon(dir: &std::path::Path) -> String {
    if cfg!(windows) {
        return "hostname".into();
    }
    let script = dir.join("dungeon.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\n\
         printf 'dungeon in %s, %s\\n' \"$PWD\" \"${CARGO_PKG_NAME:-clean}\"\n\
         [ -f dsave.dat ] && printf 'restored: %s\\n' \"$(cat dsave.dat)\"\n\
         printf '>'\n\
         while read cmd; do\n\
           case \"$cmd\" in\n\
             save*) printf 'kept at %s' \"$cmd\" > dsave.dat; printf 'Saved.\\n>';;\n\
             quit*) exit 0;;\n\
             *) printf 'Huh?\\n>';;\n\
           esac\n\
         done\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script.display().to_string()
}

impl Drop for Board {
    fn drop(&mut self) {
        let (base, name) = (self.base.clone(), self.name.clone());
        blocking(move || {
            if let Ok(mut admin) = Client::connect(&base, NoTls) {
                let _ = admin.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"));
            }
        });
    }
}

struct Dialer;

impl client::Handler for Dialer {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// A caller on the line: what they've heard and not yet read kept.
struct Caller {
    channel: russh::Channel<client::Msg>,
    heard: String,
}

impl Caller {
    /// Calls as Flow's `dial` does, its variables saying it's Flow, and waits
    /// for the welcome's menu.
    async fn dial(board: &Board) -> Caller {
        let mut caller = Caller::line(board, &[("TERM_PROGRAM", "flow-terminal")]).await;
        caller.until(MENU_KEYS).await;
        caller
    }

    /// Logs in as the guest with a pty and `env`, and opens the shell.
    async fn line(board: &Board, env: &[(&str, &str)]) -> Caller {
        let mut session = client::connect(Arc::new(client::Config::default()), board.address, Dialer).await.unwrap();
        assert!(session.authenticate_none("guest").await.unwrap().success());
        let channel = session.channel_open_session().await.unwrap();
        channel.request_pty(false, "xterm-256color", 100, 40, 0, 0, &[]).await.unwrap();
        for (name, value) in env {
            channel.set_env(false, *name, *value).await.unwrap();
        }
        channel.request_shell(true).await.unwrap();
        std::mem::forget(session);
        Caller { channel, heard: String::new() }
    }

    /// What the board says, up to and including `text`.
    async fn until(&mut self, text: &str) -> String {
        let read = async {
            while !self.heard.contains(text) {
                match self.channel.wait().await {
                    Some(ChannelMsg::Data { data }) => self.heard.push_str(&String::from_utf8_lossy(&data)),
                    Some(_) => {}
                    None => break,
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(20), read).await.unwrap_or_else(|_| panic!("waited for {text:?}, heard {:?}", self.heard));
        let end = self.heard.find(text).unwrap_or_else(|| panic!("hung up before {text:?}, heard {:?}", self.heard)) + text.len();
        let said = self.heard[..end].to_string();
        self.heard.drain(..end);
        said
    }

    async fn send(&mut self, keys: &str) {
        self.channel.data(keys.as_bytes()).await.unwrap();
    }

    /// Types `line` and Enter once `prompt` comes.
    async fn answer(&mut self, prompt: &str, line: &str) -> String {
        let said = self.until(prompt).await;
        self.send(&format!("{line}\r")).await;
        said
    }

    /// Presses a key once the board pauses.
    async fn go_on(&mut self) -> String {
        let said = self.until("Press any key to go on.").await;
        self.send(" ").await;
        said
    }

    /// Logs in, through the bulletin, to the member's menu.
    async fn log_in(&mut self, handle: &str, password: &str, code: &str) -> String {
        self.send("l").await;
        self.answer(" Handle: ", handle).await;
        self.answer(" Password: ", password).await;
        self.answer("(or a recovery code): ", code).await;
        let said = self.go_on().await;
        self.until(MENU_KEYS).await;
        said
    }
}

/// `text` without its escape sequences: CSIs to their final byte, OSC,
/// DCS and APC strings to their end, and ESC with one more (ESC 7, ESC 8).
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']' | 'P' | '_') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The code for the step after `last`, so it's never one already taken;
/// waits for the clock where it's run ahead of it.
async fn next_code(seed: &[u8], last: &mut u64) -> String {
    loop {
        let now = crypto::step_now();
        let step = (*last + 1).max(now.saturating_sub(1));
        if step <= now + 1 {
            *last = step;
            return crypto::code(seed, step, 6);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Applies as `handle`, through the email's code and the authenticator,
/// and gives the seed, the step its first code took, and the recovery codes.
async fn apply(board: &Board, caller: &mut Caller, handle: &str, password: &str) -> (Vec<u8>, u64, Vec<String>) {
    let email = format!("{handle}@example.com");
    caller.send("a").await;
    caller.answer("Do you agree to the board's rules? (y/N) ", "y").await;
    caller.answer(" Handle (what the board calls you): ", handle).await;
    caller.answer(" Your name: ", &format!("{handle} Tester")).await;
    caller.answer(" Your email address: ", &email).await;
    caller.answer(" New password (10 letters or more): ", password).await;
    caller.answer(" The same again: ", password).await;
    caller.until(" The code from the mail (R sends another): ").await;
    caller.send(&format!("{}\r", board.code_for(&email))).await;
    let said = caller.until(" The 6-digit code your app shows: ").await;
    let at = said.find("Or type it in: ").expect("the seed as text") + 15;
    let text: String = said[at..].split('\x1b').next().unwrap().chars().filter(|c| !c.is_whitespace()).collect();
    let seed = data_encoding::BASE32_NOPAD.decode(text.as_bytes()).unwrap();
    assert!(said.contains("\x1b[30;107m"), "the QR code");
    let mut step = crypto::step_now().saturating_sub(1);
    caller.send(&format!("{}\r", next_code(&seed, &mut step).await)).await;
    let said = caller.go_on().await;
    assert!(said.contains("Your recovery codes"), "{said:?}");
    let recovery: Vec<String> = plain(&said).split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).filter(|w| w.len() == 11 && w.as_bytes()[5] == b'-').map(str::to_string).collect();
    assert_eq!(recovery.len(), 8, "{said:?}");
    let said = caller.go_on().await;
    assert!(said.contains("Your application is with the sysop"), "{said:?}");
    caller.until(MENU_KEYS).await;
    (seed, step, recovery)
}

#[tokio::test]
async fn an_applicant_is_let_in_by_the_sysop_then_posts_and_files_a_bug_that_staff_triage() {
    let Some(board) = Board::new("approval") else { return };

    // Bryan applies, and is made the sysop on the board's machine.
    let mut bryan = Caller::dial(&board).await;
    let (bryan_seed, mut bryan_step, _) = apply(&board, &mut bryan, "bryan", "correct horse battery").await;
    assert!(board.sql(|db| db::make_sysop(db, "bryan").unwrap()));

    // Alice applies, and waits.
    let mut alice = Caller::dial(&board).await;
    let (alice_seed, mut alice_step, alice_recovery) = apply(&board, &mut alice, "alice", "alice's long password").await;
    let (subject, body) = board.last_mail("bryan@example.com");
    assert_eq!(subject, "Anie Solutions BBS: alice has applied", "the sysop is told: {body}");
    let said = alice.log_in_refused("alice", "alice's long password", &next_code(&alice_seed, &mut alice_step).await).await;
    assert!(said.contains("still with the sysop"), "{said:?}");

    // The sysop approves her.
    let said = bryan.log_in("bryan", "correct horse battery", &next_code(&bryan_seed, &mut bryan_step).await).await;
    assert!(plain(&said).contains("1 application waiting for you"), "{said:?}");
    bryan.send("s").await;
    bryan.until(MENU_KEYS).await;
    bryan.send("a").await;
    let said = bryan.answer(" Number to decide, Q to go back: ", "1").await;
    assert!(said.contains("alice") && said.contains("alice@example.com"), "{said:?}");
    bryan.answer("R to turn down, Enter to leave it: ", "a").await;
    let said = bryan.go_on().await;
    assert!(said.contains("alice is a member now"), "{said:?}");
    assert_eq!(board.last_mail("alice@example.com").0, "Anie Solutions BBS: welcome, alice");
    bryan.go_on().await;
    bryan.until(MENU_KEYS).await;
    bryan.send("b").await;
    bryan.until(MENU_KEYS).await;

    // He starts a thread in General.
    bryan.send("f").await;
    bryan.answer(" Number to read, Q to go back: ", "2").await;
    bryan.answer("Q to go back: ", "n").await;
    bryan.answer(" Title: ", "Hello, board").await;
    bryan.until(EDIT_KEYS).await;
    bryan.send("First post! Welcome, everyone.\x10").await;
    let said = bryan.until("goes back to writing").await;
    assert!(plain(&said).contains("bryan now") && said.contains(" First post! Welcome, everyone."), "previewed as the thread shows a post: {said:?}");
    bryan.send(" ").await;
    bryan.until(EDIT_KEYS).await;
    bryan.send("\x13").await;
    assert!(bryan.go_on().await.contains("Posted."));
    bryan.answer("Q to go back: ", "q").await;
    bryan.answer(" Number to read, Q to go back: ", "q").await;
    bryan.until(MENU_KEYS).await;

    // Alice logs in with a recovery code, finds it new, reads it and replies,
    // her escape left out on the way in.
    let said = alice.log_in_with_recovery("alice", "alice's long password", &alice_recovery[0].to_uppercase()).await;
    assert!(plain(&said).contains("1 thread with posts you haven't read."), "{said:?}");
    alice.send("f").await;
    let said = alice.answer(" Number to read, Q to go back: ", "2").await;
    assert!(said.contains("1 new"), "{said:?}");
    assert!(!said.contains("Staff room"), "a member doesn't see staff's forum: {said:?}");
    alice.answer("Q to go back: ", "1").await;
    let said = alice.answer("Q to go back: ", "r").await;
    assert!(said.contains("First post! Welcome, everyone.") && said.contains("bryan"), "{said:?}");
    alice.until(EDIT_KEYS).await;
    alice.send("Hi bryan\x1b[2J!\x13").await;
    let said = alice.answer("Q to go back: ", "").await;
    assert!(said.contains("Hi bryan!"), "the reply, its escape left out: {said:?}");
    alice.answer("Q to go back: ", "q").await;
    alice.answer("Q to go back: ", "q").await;
    alice.answer(" Number to read, Q to go back: ", "q").await;

    // She files a bug.
    alice.until(MENU_KEYS).await;
    alice.send("b").await;
    alice.answer("Q to go back: ", "n").await;
    alice.answer(" Which product (its number)? ", "1").await;
    alice.answer(" In a line, what's wrong? ", "The welcome screen takes a while").await;
    alice.answer(" How bad is it? (normal) ", "").await;
    alice.until(EDIT_KEYS).await;
    alice.send("I call, and wait.\x13").await;
    assert!(alice.go_on().await.contains("Filed as bug #1"));

    // The sysop (staff, and more) confirms it, and she's mailed.
    bryan.send("b").await;
    let said = bryan.answer("Q to go back: ", "1").await;
    assert!(said.contains("The welcome screen takes a while"), "{said:?}");
    bryan.answer("Enter to read again, Q to go back: ", "s").await;
    bryan.answer(" Its status? (new) ", "2").await;
    let said = bryan.answer("Enter to read again, Q to go back: ", "q").await;
    assert!(said.contains("status: new → confirmed"), "{said:?}");
    assert_eq!(board.last_mail("alice@example.com").0, "Anie Solutions BBS: bug #1 is confirmed");

    let (actions, used) = board.sql(|db| {
        let actions: Vec<String> = db.query("SELECT action FROM audit ORDER BY id", &[]).unwrap().iter().map(|r| r.get(0)).collect();
        let used: i64 = db.query_one("SELECT count(*) FROM recovery_codes WHERE used_at IS NOT NULL", &[]).unwrap().get(0);
        (actions, used)
    });
    for a in ["applied", "authenticator set up", "application complete", "made sysop", "login", "application approved", "bug filed", "bug changed"] {
        assert!(actions.iter().any(|x| x == a), "{a} in the log: {actions:?}");
    }
    assert_eq!(used, 1, "the recovery code is used up");
}

impl Caller {
    /// Logs in, and is told something instead of reaching the menu.
    async fn log_in_refused(&mut self, handle: &str, password: &str, code: &str) -> String {
        self.send("l").await;
        self.answer(" Handle: ", handle).await;
        self.answer(" Password: ", password).await;
        self.answer("(or a recovery code): ", code).await;
        let said = self.go_on().await;
        self.until(MENU_KEYS).await;
        said
    }

    /// Logs in with a recovery code: told how many are left, then the menu.
    async fn log_in_with_recovery(&mut self, handle: &str, password: &str, code: &str) -> String {
        self.send("l").await;
        self.answer(" Handle: ", handle).await;
        self.answer(" Password: ", password).await;
        self.answer("(or a recovery code): ", code).await;
        let said = self.go_on().await;
        assert!(said.contains("You used a recovery code; 7 are left."), "{said:?}");
        let said = self.go_on().await;
        self.until(MENU_KEYS).await;
        said
    }
}

#[tokio::test]
async fn a_login_wrong_in_any_part_says_the_same_a_code_is_good_once_and_five_wrong_make_the_handle_wait() {
    let Some(board) = Board::new("logins") else { return };
    let seed = crypto::new_seed();
    board.member("carol", "carol's good password", &seed, "member");
    let mut last = crypto::step_now().saturating_sub(2);
    let mut carol = Caller::dial(&board).await;
    let wrong = "That handle, password or code isn't right.";

    let said = carol.log_in_refused("carol", "not her password", &next_code(&seed, &mut last).await).await;
    assert!(said.contains(wrong), "wrong password: {said:?}");
    let said = carol.log_in_refused("carol", "carol's good password", "000000").await;
    assert!(said.contains(wrong), "wrong code: {said:?}");
    let said = carol.log_in_refused("nobody", "carol's good password", "123456").await;
    assert!(said.contains(wrong), "no such handle: {said:?}");

    let code = next_code(&seed, &mut last).await;
    carol.log_in("carol", "carol's good password", &code).await;
    carol.send("q").await;
    let mut carol = Caller::dial(&board).await;
    let said = carol.log_in_refused("carol", "carol's good password", &code).await;
    assert!(said.contains(wrong), "the same code again: {said:?}");

    for _ in 0..4 {
        carol.log_in_refused("carol", "not her password", "000000").await;
    }
    let said = carol.log_in_refused("carol", "carol's good password", &next_code(&seed, &mut last).await).await;
    assert!(said.contains(wrong), "right, but waiting out five wrong in a row: {said:?}");
    let locked: bool = board.sql(|db| db.query_one("SELECT locked_until > now() FROM members WHERE handle = 'carol'", &[]).unwrap().get(0));
    assert!(locked);
}

#[tokio::test]
async fn an_address_already_a_members_is_told_by_mail_and_that_application_never_comes_right() {
    let Some(board) = Board::new("taken") else { return };
    board.member("dave", "dave's good password", &crypto::new_seed(), "member");
    let mut caller = Caller::dial(&board).await;
    caller.send("a").await;
    caller.answer("Do you agree to the board's rules? (y/N) ", "y").await;
    caller.answer(" Handle (what the board calls you): ", "dave2").await;
    caller.answer(" Your name: ", "Not Dave").await;
    caller.answer(" Your email address: ", "DAVE@example.com").await;
    caller.answer(" New password (10 letters or more): ", "someone else's").await;
    caller.answer(" The same again: ", "someone else's").await;
    let said = caller.answer(" The code from the mail (R sends another): ", "123456").await;
    assert!(said.contains("A code is on its way to DAVE@example.com"), "the same steps: {said:?}");
    let said = caller.until(" The code from the mail").await;
    assert!(said.contains("That code isn't right."), "{said:?}");
    let (subject, body) = board.last_mail("DAVE@example.com");
    assert_eq!(subject, "Anie Solutions BBS: someone used your address");
    assert!(!body.chars().any(|c| c.is_ascii_digit()), "no code in it: {body}");
    let dave2: i64 = board.sql(|db| db.query_one("SELECT count(*) FROM members WHERE handle = 'dave2'", &[]).unwrap().get(0));
    assert_eq!(dave2, 0, "no member made");
}

#[tokio::test]
async fn a_forgotten_password_is_set_anew_by_a_code_to_the_members_address() {
    let Some(board) = Board::new("forgot") else { return };
    let seed = crypto::new_seed();
    board.member("erin", "erin's old password", &seed, "member");
    let mut last = crypto::step_now().saturating_sub(2);
    let mut erin = Caller::dial(&board).await;
    erin.send("f").await;
    let said = erin.answer(" Handle: ", "erin").await;
    assert!(said.contains("Not a member yet?"), "{said:?}");
    erin.answer(" Email address: ", "erin@example.com").await;
    let said = erin.until(" The code from the mail (R sends another): ").await;
    assert!(said.contains("If that handle and address are a member's, a code is on its way"), "said as an if: {said:?}");
    erin.send(&format!("{}\r", board.code_for("erin@example.com"))).await;
    erin.answer(" New password (10 letters or more): ", "erin's new password").await;
    erin.answer(" The same again: ", "erin's new password").await;
    assert!(erin.go_on().await.contains("Your password is changed."));
    assert_eq!(board.last_mail("erin@example.com").0, "Anie Solutions BBS: your password was changed");
    erin.until(MENU_KEYS).await;
    let said = erin.log_in("erin", "erin's new password", &next_code(&seed, &mut last).await).await;
    assert!(said.contains("The bulletin"), "{said:?}");
}

impl Caller {
    /// Waits for a door to open: on Windows its pty asks the caller's
    /// terminal where its cursor is first, and is answered.
    async fn door_opened(&mut self) {
        if cfg!(windows) {
            self.until("\x1b[6n").await;
            self.send("\x1b[1;1R").await;
        }
    }
}

#[tokio::test]
async fn a_guest_finds_the_demonstrations_and_the_game_room_where_neko_mines_and_dungeon_hand_the_call_back() {
    let Some(board) = Board::new("games") else { return };
    let mut guest = Caller::dial(&board).await;

    guest.send("d").await;
    let text = plain(&guest.until(MENU_KEYS).await);
    assert!(text.contains("╭─ Demonstrations ─"), "{text:?}");
    for item in ["G  The gallery", "T  The teapot, as OpenGL drew it", "H  A hypercube, turning", "L  Light sources", "F  Typeset fonts", "B  Back"] {
        assert!(text.contains(item), "{item:?} in {text:?}");
    }

    guest.send("g").await;
    let said = guest.go_on().await;
    assert!(said.contains("_Ga=T"), "the picture, sent to Flow: {said:?}");
    assert!(said.contains("_ratty;g;r;id="), "the crystal, registered");
    assert!(said.contains("7731;fx=sound;sound=chime"), "the sound");
    assert!(!said.contains("[Flow BBS, a picture]"), "no words in brackets for Flow");
    guest.until(MENU_KEYS).await;

    // The models the board makes: each moves itself, so Flow isn't asked
    // to turn it as well.
    for (key, name) in [("t", "The Utah teapot"), ("h", "A hypercube turning"), ("l", "Three colored lamps")] {
        guest.send(key).await;
        let said = guest.go_on().await;
        assert!(said.contains(&format!("name={name}")), "{name}, registered");
        let placed = &said[said.find("_ratty;g;p;").expect("placed")..];
        let placed = &placed[..placed.find('\x1b').unwrap()];
        assert!(!placed.contains("animate"), "{name} moves itself: {placed:?}");
        guest.until(MENU_KEYS).await;
    }
    guest.send("f").await;
    let said = guest.go_on().await;
    assert!(said.contains("_Ga=T") && !said.contains("[A page of type"), "the page of type, a picture: {said:.300}");
    guest.until(MENU_KEYS).await;
    guest.send("b").await;
    guest.until(MENU_KEYS).await;

    guest.send("g").await;
    let said = guest.until(MENU_KEYS).await;
    let text = plain(&said);
    assert!(text.contains("The game room") && text.contains("╭─ Game room ─"), "{text:?}");
    for item in ["M  Mines: clear the field", "L  The Mines leaderboard", "D  Dungeon: the mainframe original", "N  Neko, the cat that chases your pointer", "B  Back"] {
        assert!(text.contains(item), "{item:?} in {text:?}");
    }

    guest.send("n").await;
    guest.door_opened().await;
    let said = guest.until(MENU_KEYS).await;
    assert!(!said.contains("won't come out"), "the door opened, ended, and handed the call back: {said:?}");
    assert!(said.contains("\x1b[0m\x1b[?25h"), "plain colors and the cursor after it");

    // A guest's game: their time is theirs, and the board keeps none.
    guest.send("m").await;
    guest.door_opened().await;
    if cfg!(unix) {
        guest.answer("time: ", "42.5").await;
        let said = plain(&guest.go_on().await);
        assert!(said.contains("You cleared Small in 0:42.5.") && said.contains("apply from the welcome"), "{said:?}");
    }
    let said = guest.until(MENU_KEYS).await;
    assert!(!said.contains("isn't here"), "{said:?}");
    assert_eq!(board.sql(|db| db.query_one("SELECT count(*) FROM mines_best", &[]).unwrap().get::<_, i64>(0)), 0);

    guest.send("l").await;
    let said = plain(&guest.go_on().await);
    assert!(said.contains("The Mines leaderboard") && said.contains("No times yet: be the first."), "{said:?}");
    guest.until(MENU_KEYS).await;

    // A guest's Dungeon runs in a folder of the call's, which goes when the
    // game ends; and like every door it has none of the board's variables.
    guest.send("d").await;
    guest.door_opened().await;
    if cfg!(unix) {
        let said = plain(&guest.until(", clean\r\n").await);
        assert!(said.contains("A guest's game isn't kept"), "{said:?}");
        let folder = said.rsplit("dungeon in ").next().unwrap().trim_end_matches(", clean\r\n").to_string();
        assert!(folder.contains("anie-bbs-dungeon-"), "{folder:?}");
        guest.send("save\r").await;
        guest.until("Saved.").await;
        guest.send("quit\r").await;
        guest.until(MENU_KEYS).await;
        assert!(!std::path::Path::new(&folder).exists(), "a guest's game isn't kept: {folder}");
    } else {
        guest.until(MENU_KEYS).await;
    }

    guest.send("b").await;
    guest.until(MENU_KEYS).await;
    guest.send("q").await;
    guest.until("Goodbye.").await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_members_dungeon_is_kept_in_a_folder_of_their_own_and_taken_up_on_another_call() {
    let Some(board) = Board::new("dungeon") else { return };
    let seed = crypto::new_seed();
    let id = board.member("cleo", "cleo's long password", &seed, "member");
    let mut step = crypto::step_now().saturating_sub(1);

    let mut cleo = Caller::dial(&board).await;
    cleo.log_in("cleo", "cleo's long password", &next_code(&seed, &mut step).await).await;
    cleo.send("g").await;
    cleo.until(MENU_KEYS).await;
    cleo.send("d").await;
    let said = plain(&cleo.until(", clean\r\n").await);
    assert!(said.contains("SAVE keeps your game here for next time"), "{said:?}");
    let mine = board.saves.join("dungeon").join(id.to_string());
    assert!(said.contains(&format!("dungeon in {}", mine.display())), "in her own folder, {}: {said:?}", mine.display());
    cleo.send("save\r").await;
    cleo.until("Saved.").await;
    cleo.send("quit\r").await;
    cleo.until(MENU_KEYS).await;
    assert!(mine.join("dsave.dat").exists(), "her game, kept");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&mine).unwrap().permissions().mode() & 0o777, 0o700, "the board's alone");

    // On another call, the game she kept is there to take up.
    let mut again = Caller::dial(&board).await;
    again.log_in("cleo", "cleo's long password", &next_code(&seed, &mut step).await).await;
    again.send("g").await;
    again.until(MENU_KEYS).await;
    again.send("d").await;
    let said = again.until("\r\n>").await;
    assert!(said.contains("restored: kept at save"), "{said:?}");
    again.send("quit\r").await;
    again.until(MENU_KEYS).await;
}

#[cfg(unix)]
#[tokio::test]
async fn members_race_at_mines_their_bests_kept_and_ranked_on_the_leaderboard() {
    let Some(board) = Board::new("mines") else { return };
    let (seed_a, seed_b) = (crypto::new_seed(), crypto::new_seed());
    board.member("anna", "anna's long password", &seed_a, "member");
    board.member("ben", "ben's long password", &seed_b, "member");
    let (mut step_a, mut step_b) = (crypto::step_now().saturating_sub(1), crypto::step_now().saturating_sub(1));

    // Anna clears Small in 30.5: her first, so a best, and first place.
    let mut anna = Caller::dial(&board).await;
    anna.log_in("anna", "anna's long password", &next_code(&seed_a, &mut step_a).await).await;
    anna.send("g").await;
    anna.until(MENU_KEYS).await;
    anna.send("m").await;
    let said = anna.answer("time: ", "30.5").await;
    assert!(said.contains("given: ") && !said.contains("given: {"), "nothing to beat yet: {said:?}");
    let said = plain(&anna.go_on().await);
    assert!(said.contains("A new best on Small: 0:30.5, #1 of 1 on the leaderboard."), "{said:?}");
    anna.until(MENU_KEYS).await;

    // Ben clears it in 12.25, and takes first place.
    let mut ben = Caller::dial(&board).await;
    ben.log_in("ben", "ben's long password", &next_code(&seed_b, &mut step_b).await).await;
    ben.send("g").await;
    ben.until(MENU_KEYS).await;
    ben.send("m").await;
    ben.answer("time: ", "12.25").await;
    let said = plain(&ben.go_on().await);
    assert!(said.contains("A new best on Small: 0:12.2, #1 of 2 on the leaderboard."), "{said:?}");
    ben.until(MENU_KEYS).await;

    // Anna plays again, given her best to beat, and doesn't: nothing's said
    // and nothing changes.
    anna.send("m").await;
    let said = anna.answer("time: ", "31").await;
    assert!(said.contains(r#"given: {"Small":30.5}"#), "her best, for the game to say: {said:?}");
    let said = anna.until(MENU_KEYS).await;
    assert!(!plain(&said).contains("A new best"), "{said:?}");

    // The leaderboard: Ben, then Anna, her own row marked for her.
    anna.send("l").await;
    let said = plain(&anna.go_on().await);
    let ben_at = said.find("ben").unwrap_or_else(|| panic!("{said:?}"));
    let anna_at = said.find("anna").unwrap_or_else(|| panic!("{said:?}"));
    assert!(ben_at < anna_at, "the fastest first: {said:?}");
    assert!(said.contains("0:12.2") && said.contains("0:30.5") && said.contains("◂ you"), "{said:?}");
    assert!(said.contains("Medium") && said.contains("No times yet"), "{said:?}");

    let kept: Vec<(String, f64)> = board.sql(|db| db.query("SELECT m.handle, b.seconds FROM mines_best b JOIN members m ON m.id = b.member_id ORDER BY b.seconds", &[]).unwrap().iter().map(|r| (r.get(0), r.get(1))).collect());
    assert_eq!(kept, [("ben".to_string(), 12.25), ("anna".to_string(), 30.5)]);
}

#[tokio::test]
async fn flows_answer_coming_after_windows_console_has_answered_for_it_picks_nothing_at_the_welcome() {
    let Some(board) = Board::new("late_answer") else { return };
    // plain ssh from Flow on Windows: no variables, so the board asks which
    // terminal it is; the console between answers DA1 at once, and Flow's
    // own answer to XTVERSION comes after, its F a key the welcome takes.
    let mut caller = Caller::line(&board, &[]).await;
    caller.until("\x1b[>0q\x1b[c").await;
    caller.send("\x1b[?61;4;6;7;14;21;22;23;24;28;32;42c").await;
    caller.until(MENU_KEYS).await;
    caller.send("\x1bP>|Flow 0.1.35\x1b\\").await;
    caller.send("q").await;
    let said = caller.until("Goodbye.").await;
    assert!(!said.contains("Forgot your password") && !said.contains("Handle:"), "the answer picked nothing: {said:?}");
    assert!(plain(&said).contains("▸ Q  Hang up"), "the first key was the caller's own: {said:?}");
}

#[tokio::test]
async fn the_sysop_writes_the_bulletin_full_screen_and_flow_sees_it_typeset_and_others_as_text() {
    let Some(board) = Board::new("bulletin") else { return };
    let typeset = std::env::var_os("ANIE_BBS_TEST_FONTS").is_some();
    let (seed_s, seed_t) = (crypto::new_seed(), crypto::new_seed());
    board.member("sam", "sam's long password", &seed_s, "sysop");
    board.member("tia", "tia's long password", &seed_t, "member");
    let (mut step_s, mut step_t) = (crypto::step_now().saturating_sub(1), crypto::step_now().saturating_sub(1));

    // The sysop finds the bulletin there to change, cuts its line, writes
    // a headline and a point, and saves.
    let mut sam = Caller::dial(&board).await;
    sam.log_in("sam", "sam's long password", &next_code(&seed_s, &mut step_s).await).await;
    sam.send("s").await;
    sam.until(MENU_KEYS).await;
    sam.send("n").await;
    let said = sam.until(EDIT_KEYS).await;
    assert!(said.contains("The bulletin") && said.contains("Welcome to the Anie Solutions BBS"), "{said:?}");
    // In Flow, typeset beside the text (where the test has faces to set it
    // in): the card as it is, then afresh once the typing stops.
    if typeset {
        let said = sam.until("\x1b_Ga=T").await;
        assert!(said.contains("As its readers will see it"), "{said:?}");
        sam.until("\x1b[?25h").await;
    }
    sam.send("\x0b# Hello, \"board\"\r- Dungeon is open").await;
    if typeset {
        sam.until("\x1b_Ga=T").await;
        let said = sam.until("\x1b[?25h").await;
        assert!(said.contains("\x1b_Ga=d"), "the last card taken away once the new one's in: {said:?}");
    }
    // Previewed as members will see it: in Flow, typeset (where the test
    // has faces to set it in), else as text.
    sam.send("\x10").await;
    let said = sam.until("goes back to writing").await;
    if typeset {
        assert!(said.contains("\x1b_Ga=T") && plain(&said).contains("The bulletin"), "the card: {:?}", plain(&said));
    } else {
        assert!(said.contains("Preview: The bulletin") && said.contains("\x1b[1;36mHello, \"board\"") && said.contains("• Dungeon is open"), "{said:?}");
    }
    sam.send(" ").await;
    sam.until(EDIT_KEYS).await;
    sam.send("\x13").await;
    sam.until(MENU_KEYS).await;
    let saved: String = board.sql(|db| db.query_one("SELECT value FROM settings WHERE key = 'bulletin'", &[]).unwrap().get(0));
    assert_eq!(saved, "# Hello, \"board\"\n- Dungeon is open");

    // In Flow, it's typeset (where the test has faces to set it in), the
    // news under it as text.
    sam.send("b").await;
    sam.until(MENU_KEYS).await;
    sam.send("n").await;
    let said = sam.go_on().await;
    assert!(plain(&said).contains("0 threads with posts you haven't read"), "{said:?}");
    if typeset {
        assert!(said.contains("\x1b_Ga=T") && !said.contains("Dungeon is open"), "a picture: {:?}", plain(&said));
    } else {
        assert!(said.contains("Hello, \"board\"") && said.contains("• Dungeon is open"), "{said:?}");
    }

    // Another terminal gets it as text: the headline in the heading's
    // color, the point after a bullet.
    let mut tia = Caller::line(&board, &[]).await;
    tia.until(MENU_KEYS).await;
    let said = tia.log_in("tia", "tia's long password", &next_code(&seed_t, &mut step_t).await).await;
    assert!(said.contains("\x1b[1;36mHello, \"board\"\x1b[0m") && said.contains("• Dungeon is open"), "{said:?}");
    assert!(!said.contains("\x1b_G"), "no picture: {said:?}");
}
