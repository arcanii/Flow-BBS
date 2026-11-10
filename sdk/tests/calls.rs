//! Calls to a board served for real: on a port of this machine's own,
//! reached with russh's client half as `dial` will reach it, logging in as
//! the guest, asking for a pty and a shell, and reading what the board
//! says back.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use flow_bbs::{Call, Caller, Config, Key, MENU_KEYS};
use russh::client;
use russh::keys::PublicKeyOrCertificate;
use russh::ChannelMsg;

/// A board that says who's calling, then each key, until `q`.
fn board(call: &mut Call) -> std::io::Result<()> {
    let caller = match call.caller() {
        Caller::Flow { version } => format!("Flow {}", version.as_deref().unwrap_or("?")),
        Caller::Other { term, name } => format!("{term} {}", name.as_deref().unwrap_or("?")),
    };
    let size = call.size();
    call.say(&format!("Welcome, {}. {caller}, {}x{}, cell {:?}\n", call.user(), size.cols, size.rows, size.cell()))?;
    while let Some(key) = call.key()? {
        call.say(&format!("key {key:?}\n"))?;
        if key == Key::Char('q') {
            break;
        }
    }
    Ok(())
}

/// Serves `board` on a port the system picks, with a host key of its own.
fn served() -> SocketAddr {
    serving(board)
}

fn serving(board: impl flow_bbs::Board) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let host_key = std::env::temp_dir().join(format!("flow-bbs-test-{}-{}", std::process::id(), address.port())).join("host_key");
    let config = Config { host_key, ask: Duration::from_millis(500), ..Config::default() };
    std::thread::spawn(move || flow_bbs::serve_on(listener, config, board));
    address
}

/// A folder with a 64 × 32 PNG and a model, for screens to show: written
/// once, since the tests' calls run at once and read it.
fn pictures() -> std::path::PathBuf {
    static MADE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    MADE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("flow-bbs-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(64u32.to_be_bytes());
        png.extend(32u32.to_be_bytes());
        png.extend([8, 2, 0, 0, 0]);
        std::fs::write(dir.join("p.png"), png).unwrap();
        std::fs::write(dir.join("m.obj"), "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap();
        dir
    })
    .clone()
}

/// A board that shows a screen with a picture and a model, then a menu
/// whose door is a shell saying what it was given.
fn screens(call: &mut Call) -> std::io::Result<()> {
    let screen = flow_bbs::Screen::parse("{bold}Hi {user}{/}{sound chime}\n![A picture](p.png)\n![A model](m.obj)", &pictures()).unwrap();
    call.show(&screen, &[])?;
    let items = [flow_bbs::Item::new('d', "The door"), flow_bbs::Item::new('q', "Hang up")];
    loop {
        match call.menu(None, &[], &items)? {
            Some(0) => {
                // Cargo gives the server (this test) its package's name; a
                // door gets none of the server's variables but PATH and the
                // like.
                let door = flow_bbs::Door::new("/bin/sh", ["-c", "printf 'door %s %s %s %s\\n' \"${TERM_PROGRAM:-none}\" \"$TERM\" \"${CARGO_PKG_NAME:-clean}\" \"${PATH:+path}\"; stty size; read line; echo \"got $line\"; exit 3"]);
                let code = call.door(&door)?;
                call.say(&format!("door ended {code:?}\n"))?;
            }
            _ => return Ok(()),
        }
    }
}

struct Dialer;

impl client::Handler for Dialer {
    type Error = russh::Error;

    async fn check_server_key(&mut self, _: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Logs in as `user` and opens a shell, with Flow's variables where asked.
async fn dial(address: SocketAddr, env: &[(&str, &str)]) -> russh::Channel<client::Msg> {
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, address, Dialer).await.unwrap();
    assert!(session.authenticate_none("guest").await.unwrap().success());
    let channel = session.channel_open_session().await.unwrap();
    channel.request_pty(false, "xterm-256color", 100, 30, 1000, 600, &[]).await.unwrap();
    for (name, value) in env {
        channel.set_env(false, *name, *value).await.unwrap();
    }
    channel.request_shell(true).await.unwrap();
    std::mem::forget(session);
    channel
}

/// What the board says, up to and including `until`.
async fn heard(channel: &mut russh::Channel<client::Msg>, until: &str) -> String {
    let mut text = String::new();
    let read = async {
        while !text.contains(until) {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => text.push_str(&String::from_utf8_lossy(&data)),
                Some(_) => {}
                None => break,
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), read).await.unwrap_or_else(|_| panic!("waited for {until:?}, heard {text:?}"));
    text
}

/// The exit status the call ends with.
async fn ended(channel: &mut russh::Channel<client::Msg>) -> Option<u32> {
    let read = async {
        while let Some(msg) = channel.wait().await {
            if let ChannelMsg::ExitStatus { exit_status } = msg {
                return Some(exit_status);
            }
        }
        None
    };
    tokio::time::timeout(Duration::from_secs(10), read).await.unwrap()
}

#[tokio::test]
async fn a_caller_in_flow_is_known_by_dials_variables_and_their_keys_reach_the_board() {
    let mut line = dial(served(), &[("TERM_PROGRAM", "flow-terminal"), ("TERM_PROGRAM_VERSION", "0.1.34")]).await;
    let said = heard(&mut line, "\r\n").await;
    assert_eq!(said, "Welcome, guest. Flow 0.1.34, 100x30, cell Some((10, 20))\r\n");
    line.data(&b"x\x1b[A"[..]).await.unwrap();
    assert_eq!(heard(&mut line, "Up\r\n").await, "key Char('x')\r\nkey Up\r\n");
    line.window_change(120, 40, 0, 0).await.unwrap();
    assert_eq!(heard(&mut line, "Resize\r\n").await, "key Resize\r\n");
    line.data(&b"q"[..]).await.unwrap();
    assert_eq!(heard(&mut line, "\r\n").await, "key Char('q')\r\n");
    assert_eq!(ended(&mut line).await, Some(0), "the board returned, so the call ended");
}

#[tokio::test]
async fn a_caller_without_the_variables_is_asked_and_flows_answer_counts() {
    let mut line = dial(served(), &[]).await;
    assert_eq!(heard(&mut line, "\x1b[c").await, "\x1b[>0q\x1b[c", "XTVERSION, then DA1");
    line.data(&b"\x1bP>|Flow 0.1.35\x1b\\\x1b[?62;22c"[..]).await.unwrap();
    assert_eq!(heard(&mut line, "\r\n").await, "Welcome, guest. Flow 0.1.35, 100x30, cell Some((10, 20))\r\n");
}

#[tokio::test]
async fn another_terminal_that_answers_nothing_is_waited_for_no_longer_than_asked() {
    let mut line = dial(served(), &[]).await;
    heard(&mut line, "\x1b[c").await;
    let said = heard(&mut line, "\r\n").await;
    assert_eq!(said, "Welcome, guest. xterm-256color ?, 100x30, cell Some((10, 20))\r\n");
}

#[tokio::test]
async fn only_the_guest_login_is_let_in() {
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, served(), Dialer).await.unwrap();
    assert!(!session.authenticate_none("root").await.unwrap().success());
    assert!(!session.authenticate_password("root", "hunter2").await.unwrap().success());
}

#[tokio::test]
async fn a_caller_in_flow_sees_the_pictures_and_models_and_hears_the_sounds() {
    let mut line = dial(serving(screens), &[("TERM_PROGRAM", "flow-terminal")]).await;
    let said = heard(&mut line, MENU_KEYS).await;
    assert!(said.starts_with("\x1b[2J\x1b[H\x1b[1mHi guest\x1b[0m"), "{said:?}");
    assert!(said.contains("\x1b]7731;fx=sound;sound=chime"), "the sound");
    assert!(said.contains("\x1b_Ga=T"), "the picture sent: {said:?}");
    assert!(said.contains('\u{10EEEE}'), "and its placeholder cells");
    assert!(said.contains("\x1b_ratty;g;r;id=") && said.contains("\x1b_ratty;g;p;id="), "the model registered and placed");
    assert!(!said.contains("[A picture]"));
    assert!(said.contains("▸ \x1b[1mD\x1b[22m  The door"), "the items in their frame, the bar on the first: {said:?}");
}

#[tokio::test]
async fn another_caller_sees_the_words_in_brackets_and_no_escapes_of_flows() {
    let mut line = dial(serving(screens), &[("LANG", "C")]).await;
    heard(&mut line, "\x1b[c").await;
    let said = heard(&mut line, MENU_KEYS).await;
    assert!(said.contains("[A picture]") && said.contains("[A model]"), "{said:?}");
    assert!(!said.contains("7731") && !said.contains("\x1b_G") && !said.contains("ratty"), "{said:?}");
}

#[tokio::test]
async fn a_door_runs_on_a_pty_of_the_calls_own_with_flows_variables_and_hands_the_call_back() {
    let mut line = dial(serving(screens), &[("TERM_PROGRAM", "flow-terminal")]).await;
    heard(&mut line, MENU_KEYS).await;
    line.data(&b"D"[..]).await.unwrap();
    let said = heard(&mut line, "30 100").await;
    assert!(said.contains("door flow-terminal xterm-256color clean path"), "Flow's variables, the caller's TERM, PATH, and nothing of the server's: {said:?}");
    line.data(&b"hello\r"[..]).await.unwrap();
    let said = heard(&mut line, MENU_KEYS).await;
    assert!(said.contains("got hello"), "{said:?}");
    assert!(said.contains("door ended Some(3)"), "its exit code: {said:?}");
    line.data(&b"q"[..]).await.unwrap();
    assert_eq!(ended(&mut line).await, Some(0));
}

/// A board that asks a name and a password, and says what it was given.
fn asks(call: &mut Call) -> std::io::Result<()> {
    let name = call.ask("Name: ", 8)?;
    let word = call.ask_secret("Password: ", 20)?;
    call.say(&format!("got {name:?} {word:?}\n"))?;
    Ok(())
}

#[tokio::test]
async fn a_typed_line_is_echoed_a_password_starred_and_nothing_but_letters_kept() {
    let mut line = dial(serving(asks), &[("TERM_PROGRAM", "flow-terminal")]).await;
    heard(&mut line, "Name: ").await;
    line.data(&b"bry\x1b[31mx\x7fan\r"[..]).await.unwrap();
    let said = heard(&mut line, "Password: ").await;
    assert!(said.starts_with("bryx\x08 \x08an\r\n"), "the escape left out, the rest echoed and backed over: {said:?}");
    line.data(&b"s3cret\r"[..]).await.unwrap();
    let said = heard(&mut line, "s3cret\")").await;
    assert!(said.starts_with("******\r\n"), "a star a letter: {said:?}");
    assert!(said.contains(r#"got Some("bryan") Some("s3cret")"#), "{said:?}");
}

#[tokio::test]
async fn a_line_given_up_with_esc_is_none_and_too_long_a_line_stops_at_its_limit() {
    let mut line = dial(serving(asks), &[("TERM_PROGRAM", "flow-terminal")]).await;
    heard(&mut line, "Name: ").await;
    line.data(&b"abcdefghijk\r"[..]).await.unwrap();
    heard(&mut line, "Password: ").await;
    line.data(&b"x\x1b"[..]).await.unwrap();
    let said = heard(&mut line, "None").await;
    assert!(said.contains(r#"Some("abcdefgh") None"#), "{said:?}");
}
