//! How the board is set up, from its environment (a compose file's
//! `env_file`, on tkaniedev03), checked whole when it starts:
//!
//! | Variable | What it is |
//! | --- | --- |
//! | `ANIE_BBS_DATABASE_URL` | Its PostgreSQL, as `postgres://user:password@host:port/db` (required) |
//! | `ANIE_BBS_SECRET_KEY` | 64 hex digits: seals the authenticator seeds and keys the codes' hashes (required) |
//! | `ANIE_BBS_ADDRESS` | Where it listens (`0.0.0.0:2222`) |
//! | `ANIE_BBS_HOST_KEY` | Its SSH host key, made there the first time (`host_key`) |
//! | `ANIE_BBS_SCREENS` | The folder of its screens and their pictures (`screens`) |
//! | `ANIE_BBS_NEKO` | Neko's door, the game room's cat (`flow-cat`, on the PATH) |
//! | `ANIE_BBS_MINES` | Mines' door, the game room's (`flow-mines`, on the PATH) |
//! | `ANIE_BBS_NAME` | Its name (`Anie Solutions BBS`) |
//! | `ANIE_BBS_TIME_ZONE` | The zone its times are shown in (`Asia/Tokyo`) |
//! | `ANIE_BBS_SYSOP_MAIL` | Where news of an application goes; else to the sysops' own addresses |
//! | `ANIE_BBS_MAX_CALLS`, `ANIE_BBS_MAX_CALLS_PER_ADDRESS` | Connections at once (64), and from one address (4) |
//! | `SMTP_*` | The mail relay (mail.rs) |
//! | `ANIE_BBS_MAIL=dev` | Mail to the log instead: a board on one machine only |

use std::path::PathBuf;

use crate::crypto::Keys;
use crate::mail::Smtp;

/// The board's settings. The database URL holds a password, so there's no
/// `Debug`.
pub struct Settings {
    pub name: String,
    pub address: String,
    pub host_key: PathBuf,
    pub database_url: String,
    pub keys: Keys,
    pub screens: PathBuf,
    pub neko: String,
    pub mines: String,
    pub zone: String,
    pub sysop_mail: Option<String>,
    pub max_calls: usize,
    pub max_calls_per_address: usize,
}

/// How the board's mail goes.
pub enum Mail {
    Smtp(Smtp),
    Dev,
}

impl Settings {
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<(Settings, Mail), String> {
        let present = |k: &str| get(k).map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
        let need = |k: &str| present(k).ok_or(format!("{k} must be set"));
        let number = |k: &str, default: usize| match present(k) {
            Some(n) => n.parse().map_err(|_| format!("{k} must be a number")),
            None => Ok(default),
        };
        let zone = present("ANIE_BBS_TIME_ZONE").unwrap_or("Asia/Tokyo".into());
        if !zone.chars().all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c)) {
            return Err("ANIE_BBS_TIME_ZONE must be a zone's name, as Asia/Tokyo".into());
        }
        let settings = Settings {
            name: present("ANIE_BBS_NAME").unwrap_or("Anie Solutions BBS".into()),
            address: present("ANIE_BBS_ADDRESS").unwrap_or("0.0.0.0:2222".into()),
            host_key: present("ANIE_BBS_HOST_KEY").unwrap_or("host_key".into()).into(),
            database_url: need("ANIE_BBS_DATABASE_URL")?,
            keys: Keys::from_hex(&need("ANIE_BBS_SECRET_KEY")?)?,
            screens: present("ANIE_BBS_SCREENS").unwrap_or("screens".into()).into(),
            neko: present("ANIE_BBS_NEKO").unwrap_or("flow-cat".into()),
            mines: present("ANIE_BBS_MINES").unwrap_or("flow-mines".into()),
            zone,
            sysop_mail: present("ANIE_BBS_SYSOP_MAIL"),
            max_calls: number("ANIE_BBS_MAX_CALLS", 64)?,
            max_calls_per_address: number("ANIE_BBS_MAX_CALLS_PER_ADDRESS", 4)?,
        };
        let mail = match (Smtp::from_lookup(&get)?, present("ANIE_BBS_MAIL").as_deref()) {
            (Some(_), Some("dev")) => return Err("ANIE_BBS_MAIL=dev and SMTP_HOST are both set: choose one".into()),
            (Some(smtp), _) => Mail::Smtp(smtp),
            (None, Some("dev")) => Mail::Dev,
            (None, _) => return Err("SMTP_HOST must be set, so the board can mail its codes (or ANIE_BBS_MAIL=dev, for a board on this machine alone)".into()),
        };
        Ok((settings, mail))
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

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn a_board_needs_its_database_its_key_and_a_way_to_mail() {
        let e = Settings::from_lookup(env(&[])).err().unwrap();
        assert!(e.contains("ANIE_BBS_DATABASE_URL"), "{e}");
        let e = Settings::from_lookup(env(&[("ANIE_BBS_DATABASE_URL", "postgres://x"), ("ANIE_BBS_SECRET_KEY", "short")])).err().unwrap();
        assert!(e.contains("64 hex digits"), "{e}");
        let e = Settings::from_lookup(env(&[("ANIE_BBS_DATABASE_URL", "postgres://x"), ("ANIE_BBS_SECRET_KEY", KEY)])).err().unwrap();
        assert!(e.contains("SMTP_HOST must be set"), "{e}");
    }

    #[test]
    fn what_isnt_set_has_a_default() {
        let (s, mail) = Settings::from_lookup(env(&[("ANIE_BBS_DATABASE_URL", "postgres://x"), ("ANIE_BBS_SECRET_KEY", KEY), ("ANIE_BBS_MAIL", "dev")])).unwrap();
        assert!(matches!(mail, Mail::Dev));
        assert_eq!((s.name.as_str(), s.address.as_str(), s.zone.as_str()), ("Anie Solutions BBS", "0.0.0.0:2222", "Asia/Tokyo"));
        assert_eq!((s.max_calls, s.max_calls_per_address), (64, 4));
        assert!(s.sysop_mail.is_none());
    }

    #[test]
    fn a_zone_is_a_name_and_nothing_more() {
        let e = Settings::from_lookup(env(&[("ANIE_BBS_TIME_ZONE", "UTC'; DROP TABLE members; --")])).err().unwrap();
        assert!(e.contains("ANIE_BBS_TIME_ZONE"), "{e}");
    }
}
