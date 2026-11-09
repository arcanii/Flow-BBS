//! Mail the board sends: codes, approvals, notices. Its relay is set as
//! United One's is (`united-one-mail`): `SMTP_HOST`, `SMTP_PORT`,
//! `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM` and `SMTP_FROM_NAME`, the
//! sender read as a mailbox when the board starts, so a relay that looks set
//! but can't send is said then, not found later. For Gmail: smtp.gmail.com,
//! port 587 (STARTTLS) or 465 (TLS), the Gmail address as the username and
//! an app password as the password.
//!
//! Nothing here prints a password or an address it was given: a relay's
//! error echoes the recipient, so a failure is logged as a word from a short
//! list (`error_class`), never as the error.

use std::sync::Mutex;
use std::time::Duration;

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

/// What sends the board's mail.
pub trait Mailer: Send + Sync {
    /// Sends `body` to `to`; an error is a word from `error_class`'s list.
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), &'static str>;
    /// What it is, for the log at the start: "smtp", "dev" or "memory".
    fn kind(&self) -> &'static str;
}

/// The relay. Its password is never printed: there's no `Debug`, so no
/// `{:?}` can show it.
#[derive(Clone)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub username: String,
    password: String,
    pub from: Mailbox,
}

impl Smtp {
    /// From `SMTP_*`: `None` when `SMTP_HOST` isn't set, an error when it is
    /// and the rest isn't, or the sender isn't a mailbox.
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Option<Smtp>, String> {
        let present = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let Some(host) = present("SMTP_HOST") else { return Ok(None) };
        let need = |k: &str| present(k).ok_or(format!("SMTP_HOST is set, so {k} must be too"));
        let port = match present("SMTP_PORT") {
            Some(p) => p.parse().map_err(|_| "SMTP_PORT must be a port number".to_string())?,
            None => 587,
        };
        let username = need("SMTP_USERNAME")?;
        // An app password is shown in fours with spaces; Gmail takes it without.
        let password: String = need("SMTP_PASSWORD")?.chars().filter(|c| !c.is_whitespace()).collect();
        let from = need("SMTP_FROM")?;
        let name = present("SMTP_FROM_NAME").unwrap_or("Anie Solutions BBS".into());
        let mailbox = format!("{name} <{from}>");
        let from = mailbox.parse::<Mailbox>().map_err(|_| format!("SMTP_FROM and SMTP_FROM_NAME don't make a sender ({mailbox:?}): SMTP_FROM is an address, SMTP_FROM_NAME the name shown"))?;
        Ok(Some(Smtp { host, port, username, password, from }))
    }

    fn transport(&self) -> Result<SmtpTransport, lettre::transport::smtp::Error> {
        let relay = if self.port == 465 { SmtpTransport::relay(&self.host)? } else { SmtpTransport::starttls_relay(&self.host)? };
        Ok(relay.port(self.port).credentials(Credentials::new(self.username.clone(), self.password.clone())).timeout(Some(Duration::from_secs(20))).build())
    }

    /// Logs in to the relay and says whether it let us: for `anie-bbs
    /// check-mail`, before the board is opened to callers.
    pub fn check(&self) -> Result<(), &'static str> {
        match self.transport().and_then(|t| t.test_connection()) {
            Ok(true) => Ok(()),
            Ok(false) => Err("connect"),
            Err(e) => Err(error_class(&format!("{e:?}"))),
        }
    }
}

impl Mailer for Smtp {
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), &'static str> {
        let to: Mailbox = to.parse().map_err(|_| "address")?;
        let message = Message::builder().from(self.from.clone()).to(to).subject(subject).body(body.to_string()).map_err(|_| "address")?;
        self.transport().and_then(|t| t.send(&message).map(|_| ())).map_err(|e| error_class(&format!("{e:?}")))
    }

    fn kind(&self) -> &'static str {
        "smtp"
    }
}

/// Mail written to the board's log instead of sent: for a board on one
/// machine, tried by its sysop alone. Never on a board others can reach,
/// since the codes are in the log.
pub struct Dev;

impl Mailer for Dev {
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), &'static str> {
        eprintln!("anie-bbs: dev mail to {to}: {subject}\n{body}\n");
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "dev"
    }
}

/// Mail kept for a test to read.
#[derive(Default)]
pub struct Memory {
    pub sent: Mutex<Vec<(String, String, String)>>,
}

impl Mailer for Memory {
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), &'static str> {
        self.sent.lock().unwrap().push((to.into(), subject.into(), body.into()));
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "memory"
    }
}

impl Mailer for std::sync::Arc<Memory> {
    fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), &'static str> {
        self.as_ref().send(to, subject, body)
    }

    fn kind(&self) -> &'static str {
        "memory"
    }
}

/// A sending error as one word, with no address in it: from the error's
/// `Debug`, since its `Display` is the relay's own line, address and all.
pub fn error_class(debug: &str) -> &'static str {
    let d = debug.to_ascii_lowercase();
    if d.contains("invalidinput") || d.contains("invalidaddress") || d.contains("missingparts") {
        "address"
    } else if d.contains("auth") || d.contains("credential") || d.contains("535") {
        "auth"
    } else if d.contains("tls") || d.contains("certificate") {
        "tls"
    } else if d.contains("timed out") || d.contains("timeout") {
        "timeout"
    } else if d.contains("connect") || d.contains("dns") || d.contains("resolve") {
        "connect"
    } else if d.contains("550") || d.contains("551") || d.contains("553") || d.contains("recipient") {
        "rejected"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn no_host_is_no_relay_and_a_host_needs_the_rest() {
        assert!(Smtp::from_lookup(env(&[])).unwrap().is_none());
        assert!(Smtp::from_lookup(env(&[("SMTP_HOST", "  ")])).unwrap().is_none(), "empty is unset");
        let e = Smtp::from_lookup(env(&[("SMTP_HOST", "smtp.gmail.com"), ("SMTP_USERNAME", "a@gmail.com")])).err().unwrap();
        assert!(e.contains("SMTP_PASSWORD"), "{e}");
    }

    #[test]
    fn a_gmail_relay_reads_with_its_app_password_spaces_taken_out() {
        let get = env(&[("SMTP_HOST", "smtp.gmail.com"), ("SMTP_USERNAME", "anie@gmail.com"), ("SMTP_PASSWORD", "abcd efgh ijkl mnop"), ("SMTP_FROM", "anie@gmail.com")]);
        let smtp = Smtp::from_lookup(get).unwrap().unwrap();
        assert_eq!((smtp.host.as_str(), smtp.port), ("smtp.gmail.com", 587));
        assert_eq!(smtp.password, "abcdefghijklmnop");
        assert_eq!(smtp.from.to_string(), "Anie Solutions BBS <anie@gmail.com>");
    }

    #[test]
    fn a_name_where_the_address_goes_is_said_at_the_start() {
        let get = env(&[("SMTP_HOST", "smtp.gmail.com"), ("SMTP_USERNAME", "u"), ("SMTP_PASSWORD", "p"), ("SMTP_FROM", "Anie Solutions BBS")]);
        let e = Smtp::from_lookup(get).err().unwrap();
        assert!(e.contains("SMTP_FROM is an address"), "{e}");
    }

    #[test]
    fn a_failure_is_one_word_with_no_address_in_it() {
        assert_eq!(error_class("Error { kind: Permanent(Code { 535 }) .. auth failed }"), "auth");
        assert_eq!(error_class("Permanent 550 5.1.1 <someone@example.com>: Recipient address rejected"), "rejected");
        assert_eq!(error_class("Connection timed out"), "timeout");
        assert_eq!(error_class("something new"), "other");
    }
}
