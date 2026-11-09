//! The board's database: PostgreSQL, a connection a call, the schema brought
//! up to date when the board starts. A database error is logged by its
//! SQLSTATE and constraint alone, never its text, which can hold what a
//! member typed (an address in a duplicate key's detail); the call is told
//! only that something went wrong on the board's side.

use std::fmt;
use std::io;

use postgres::{Client, NoTls};

/// The schema, a migration a step, in order.
const MIGRATIONS: &[(i32, &str)] = &[(1, include_str!("../migrations/0001_init.sql")), (2, include_str!("../migrations/0002_games.sql"))];

/// A key `pg_advisory_lock` takes while migrating, so two boards starting at
/// once don't both migrate.
const MIGRATING: i64 = 0x616e_6965_6262_7301;

/// Connects to the board's database, in the board's time zone.
pub fn connect(url: &str, zone: &str) -> Result<Client, postgres::Error> {
    let mut client = Client::connect(url, NoTls)?;
    client.batch_execute(&format!("SET TIME ZONE '{zone}'"))?;
    Ok(client)
}

/// Brings the schema up to date: each migration not yet applied, in a
/// transaction of its own. Gives how many were applied.
pub fn migrate(client: &mut Client) -> Result<usize, postgres::Error> {
    client.execute("SELECT pg_advisory_lock($1)", &[&MIGRATING])?;
    let done = (|| {
        client.batch_execute("CREATE TABLE IF NOT EXISTS schema_migrations (version integer PRIMARY KEY, applied_at timestamptz NOT NULL DEFAULT now())")?;
        let mut applied = 0;
        for (version, sql) in MIGRATIONS {
            let have = client.query_opt("SELECT 1 FROM schema_migrations WHERE version = $1", &[version])?.is_some();
            if !have {
                let mut t = client.transaction()?;
                t.batch_execute(sql)?;
                t.execute("INSERT INTO schema_migrations (version) VALUES ($1)", &[version])?;
                t.commit()?;
                applied += 1;
            }
        }
        Ok(applied)
    })();
    client.execute("SELECT pg_advisory_unlock($1)", &[&MIGRATING])?;
    done
}

/// Makes `handle` the sysop, let in: they must have finished applying
/// (address proved, authenticator set up). Gives whether there was such a
/// member.
pub fn make_sysop(client: &mut Client, handle: &str) -> Result<bool, postgres::Error> {
    let mut t = client.transaction()?;
    let n = t.execute(
        "UPDATE members SET role = 'sysop', status = 'active', decided_at = coalesce(decided_at, now())          WHERE handle_key = lower($1) AND email_verified AND seed_sealed IS NOT NULL AND status IN ('pending', 'active')",
        &[&handle],
    )?;
    if n == 1 {
        t.execute("INSERT INTO audit (action, subject) VALUES ('made sysop', $1)", &[&handle])?;
    }
    t.commit()?;
    Ok(n == 1)
}

/// A database error as a call's error: logged without its words, carried
/// as `Broke` so the call can say sorry and end.
pub fn broke(e: postgres::Error) -> io::Error {
    eprintln!("anie-bbs: database: {}", class(&e));
    io::Error::other(Broke)
}

/// What a database error is, without what it says: its SQLSTATE and the
/// constraint it broke, or that the connection went.
pub fn class(e: &postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => format!("{} {}", db.code().code(), db.constraint().unwrap_or("")).trim().to_string(),
        None if e.is_closed() => "connection closed".into(),
        None => "connection".into(),
    }
}

/// Whether `e` is a unique constraint broken (another row has that value).
pub fn taken(e: &postgres::Error) -> bool {
    e.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION)
}

/// The mark on a call's error that the database broke.
#[derive(Debug)]
pub struct Broke;

impl fmt::Display for Broke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the database broke")
    }
}

impl std::error::Error for Broke {}

/// `?` for database results in a call.
pub trait Db<T> {
    fn db(self) -> io::Result<T>;
}

impl<T> Db<T> for Result<T, postgres::Error> {
    fn db(self) -> io::Result<T> {
        self.map_err(broke)
    }
}

/// A member's role. Each takes in the ones below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Member,
    Staff,
    Sysop,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Member, Role::Staff, Role::Sysop];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Member => "member",
            Role::Staff => "staff",
            Role::Sysop => "sysop",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// The roles this one takes in (itself and those below), for `= ANY`.
    pub fn and_below(self) -> Vec<String> {
        Role::ALL.into_iter().filter(|r| *r <= self).map(|r| r.as_str().to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_role_takes_in_the_ones_below_it() {
        assert!(Role::Member < Role::Staff && Role::Staff < Role::Sysop);
        assert_eq!(Role::Member.and_below(), ["member"]);
        assert_eq!(Role::Sysop.and_below(), ["member", "staff", "sysop"]);
        assert_eq!(Role::parse("staff"), Some(Role::Staff));
        assert_eq!(Role::parse("root"), None);
    }
}
