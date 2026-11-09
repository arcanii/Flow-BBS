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
        let env = [
            ("ANIE_BBS_DATABASE_URL", url.clone()),
            ("ANIE_BBS_SECRET_KEY", KEY.into()),
            ("ANIE_BBS_MAIL", "dev".into()),
            ("ANIE_BBS_SCREENS", concat!(env!("CARGO_MANIFEST_DIR"), "/screens").into()),
            ("ANIE_BBS_HOST_KEY", dir.join("host_key").display().to_string()),
        ];
        let (settings, _) = Settings::from_lookup(|k| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())).unwrap();
        let mail = Arc::new(Memory::default());
        let bbs = Bbs::new(settings, Box::new(mail.clone())).unwrap();
        bbs.migrate().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || anie_bbs::serve(bbs, Some(listener)));
        Some(Board { address, url, mail, name, base })
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
    async fn dial(board: &Board) -> Caller {
        let mut session = client::connect(Arc::new(client::Config::default()), board.address, Dialer).await.unwrap();
        assert!(session.authenticate_none("guest").await.unwrap().success());
        let channel = session.channel_open_session().await.unwrap();
        channel.request_pty(false, "xterm-256color", 100, 40, 0, 0, &[]).await.unwrap();
        channel.set_env(false, "TERM_PROGRAM", "flow-terminal").await.unwrap();
        channel.request_shell(true).await.unwrap();
        std::mem::forget(session);
        let mut caller = Caller { channel, heard: String::new() };
        caller.until("Your choice: ").await;
        caller
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
        self.until("Your choice: ").await;
        said
    }
}

/// `text` without its escape sequences.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
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
    caller.until("Your choice: ").await;
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
    bryan.until("Your choice: ").await;
    bryan.send("a").await;
    let said = bryan.answer(" Number to decide, Q to go back: ", "1").await;
    assert!(said.contains("alice") && said.contains("alice@example.com"), "{said:?}");
    bryan.answer("R to turn down, Enter to leave it: ", "a").await;
    let said = bryan.go_on().await;
    assert!(said.contains("alice is a member now"), "{said:?}");
    assert_eq!(board.last_mail("alice@example.com").0, "Anie Solutions BBS: welcome, alice");
    bryan.go_on().await;
    bryan.until("Your choice: ").await;
    bryan.send("b").await;
    bryan.until("Your choice: ").await;

    // He starts a thread in General.
    bryan.send("f").await;
    bryan.answer(" Number to read, Q to go back: ", "2").await;
    bryan.answer("Q to go back: ", "n").await;
    bryan.answer(" Title: ", "Hello, board").await;
    bryan.answer("  1:", "First post! Welcome, everyone.").await;
    bryan.answer("  2:", "/s").await;
    assert!(bryan.go_on().await.contains("Posted."));
    bryan.answer("Q to go back: ", "q").await;
    bryan.answer(" Number to read, Q to go back: ", "q").await;
    bryan.until("Your choice: ").await;

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
    alice.answer("  1:", "Hi bryan\x1b[2J!").await;
    alice.answer("  2:", "/s").await;
    let said = alice.answer("Q to go back: ", "").await;
    assert!(said.contains("Hi bryan!"), "the reply, its escape left out: {said:?}");
    alice.answer("Q to go back: ", "q").await;
    alice.answer("Q to go back: ", "q").await;
    alice.answer(" Number to read, Q to go back: ", "q").await;

    // She files a bug.
    alice.until("Your choice: ").await;
    alice.send("b").await;
    alice.answer("Q to go back: ", "n").await;
    alice.answer(" Which product (its number)? ", "1").await;
    alice.answer(" In a line, what's wrong? ", "The welcome screen takes a while").await;
    alice.answer(" How bad is it? (normal) ", "").await;
    alice.answer("  1:", "I call, and wait.").await;
    alice.answer("  2:", "/s").await;
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
        self.until("Your choice: ").await;
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
        self.until("Your choice: ").await;
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
    erin.until("Your choice: ").await;
    let said = erin.log_in("erin", "erin's new password", &next_code(&seed, &mut last).await).await;
    assert!(said.contains("The bulletin"), "{said:?}");
}
