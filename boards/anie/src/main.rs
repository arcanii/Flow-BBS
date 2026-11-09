//! `anie-bbs`, the Anie Solutions BBS, set up from its environment
//! (settings.rs):
//!
//! ```text
//! anie-bbs                  migrate the database, then serve the board
//! anie-bbs migrate          migrate the database, and stop
//! anie-bbs sysop <handle>   make a member who's applied the board's sysop, let in
//! anie-bbs check-mail       log in to the mail relay, and say whether it let us
//! ```
//!
//! A board starts with no sysop: the sysop applies as anyone does, then
//! `anie-bbs sysop` on the board's machine makes them one.

use std::process::ExitCode;

use anie_bbs::mail::{Dev, Mailer};
use anie_bbs::settings::{Mail, Settings};
use anie_bbs::{db, Bbs};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (settings, mail) = match Settings::from_lookup(|k| std::env::var(k).ok()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("anie-bbs: {e}");
            return ExitCode::FAILURE;
        }
    };
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => serve(settings, mail),
        ["migrate"] => migrate(&settings),
        ["sysop", handle] => sysop(&settings, handle),
        ["check-mail"] => check_mail(&mail),
        _ => {
            eprintln!("usage: anie-bbs [migrate | sysop <handle> | check-mail]");
            ExitCode::from(2)
        }
    }
}

fn migrate(settings: &Settings) -> ExitCode {
    match db::connect(&settings.database_url, &settings.zone).and_then(|mut c| db::migrate(&mut c)) {
        Ok(n) => {
            eprintln!("anie-bbs: migrate: {n} applied");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("anie-bbs: migrate: {}", db::class(&e));
            ExitCode::FAILURE
        }
    }
}

fn serve(settings: Settings, mail: Mail) -> ExitCode {
    let mailer: Box<dyn Mailer> = match mail {
        Mail::Smtp(smtp) => {
            eprintln!("anie-bbs: mail by {}:{} as {}", smtp.host, smtp.port, smtp.from.email);
            Box::new(smtp)
        }
        Mail::Dev => {
            eprintln!("anie-bbs: ANIE_BBS_MAIL=dev: codes go to this log, not by mail. Never on a board others can reach.");
            Box::new(Dev)
        }
    };
    let bbs = match Bbs::new(settings, mailer) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("anie-bbs: the screens: {e}");
            return ExitCode::FAILURE;
        }
    };
    match bbs.migrate() {
        Ok(n) => eprintln!("anie-bbs: database ready ({n} migrations applied)"),
        Err(e) => {
            eprintln!("anie-bbs: database: {}", db::class(&e));
            return ExitCode::FAILURE;
        }
    }
    eprintln!("anie-bbs: {} on {}", bbs.settings.name, bbs.settings.address);
    match anie_bbs::serve(bbs, None) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("anie-bbs: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Makes `handle` the sysop: they must have applied, proved their address
/// and set up their authenticator.
fn sysop(settings: &Settings, handle: &str) -> ExitCode {
    match db::connect(&settings.database_url, &settings.zone).and_then(|mut c| db::make_sysop(&mut c, handle)) {
        Ok(true) => {
            eprintln!("anie-bbs: {handle} is the sysop now");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("anie-bbs: no member {handle} who has finished applying (address proved, authenticator set up)");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("anie-bbs: database: {}", db::class(&e));
            ExitCode::FAILURE
        }
    }
}

fn check_mail(mail: &Mail) -> ExitCode {
    match mail {
        Mail::Smtp(smtp) => match smtp.check() {
            Ok(()) => {
                eprintln!("anie-bbs: check-mail: {}:{} let us in as {}", smtp.host, smtp.port, smtp.username);
                ExitCode::SUCCESS
            }
            Err(class) => {
                eprintln!("anie-bbs: check-mail: {}:{} didn't: {class}", smtp.host, smtp.port);
                ExitCode::FAILURE
            }
        },
        Mail::Dev => {
            eprintln!("anie-bbs: check-mail: ANIE_BBS_MAIL=dev, so there's no relay to check");
            ExitCode::FAILURE
        }
    }
}
