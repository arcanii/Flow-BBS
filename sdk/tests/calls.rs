//! Calls to a board served for real: on a port of this machine's own,
//! reached with russh's client half as `dial` will reach it, logging in as
//! the guest, asking for a pty and a shell, and reading what the board
//! says back.

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use flow_bbs::{Call, Caller, Config, Key};
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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let host_key = std::env::temp_dir().join(format!("flow-bbs-test-{}-{}", std::process::id(), address.port())).join("host_key");
    let config = Config { host_key, ask: Duration::from_millis(500), ..Config::default() };
    std::thread::spawn(move || flow_bbs::serve_on(listener, config, board));
    address
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
