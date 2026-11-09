//! The game room (experimental), for guests and members: Neko, the cat
//! that chases the pointer, and Mines, whose members' best times make a
//! leaderboard they share. Both are doors on the server's PATH: `flow-cat`
//! (this repository's `doors/cat`) and `flow-mines` (Flow-Games').
//!
//! Mines keeps a best time for each of its sizes in `flow-mines/best.json`
//! under its data folder. Each game gets a folder of its own for that
//! (`XDG_DATA_HOME`), a member's times from the board put in it first, so
//! the game says what they have to beat and writes a time down only when
//! it's beaten; the board takes in what it wrote when the door ends. A
//! guest's times are theirs alone, and go nowhere.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flow_bbs::{Door, Item};

use crate::crypto;
use crate::db::Db;
use crate::session::Session;
use crate::ui::{self, BOLD, CYAN, DIM, GREEN, PLAIN, YELLOW};

/// Mines' sizes as it names them, the ones it keeps best times for, and
/// their fields.
pub const SIZES: [(&str, &str); 3] = [("Small", "9 × 9, 10 mines"), ("Medium", "16 × 16, 40 mines"), ("Large", "30 × 16, 99 mines")];

/// The slowest time taken in: a day. Anything else isn't a game's.
const SLOWEST: f64 = 24.0 * 3600.0;

/// How many of each size the leaderboard shows (and the caller's own,
/// where it's further down).
const SHOWN: usize = 10;

/// The best times in a Mines file, for its sizes: nothing where there's
/// no file, or it isn't Mines', and no entry that isn't a time.
pub fn bests(file: &Path) -> BTreeMap<String, f64> {
    let Ok(text) = fs::read_to_string(file) else { return BTreeMap::new() };
    let Ok(serde_json::Value::Object(kept)) = serde_json::from_str(&text) else { return BTreeMap::new() };
    SIZES
        .iter()
        .filter_map(|&(size, _)| {
            let t = kept.get(size)?.as_f64()?;
            (t.is_finite() && t > 0.0 && t < SLOWEST).then(|| (size.to_string(), t))
        })
        .collect()
}

/// A time as the leaderboard shows it, to a tenth: 1:05.3.
pub fn clock(secs: f64) -> String {
    let tenths = (secs * 10.0).floor() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// A new folder for one game's data, unlike any other's: the call's
/// number and a random tag, in the system's temporary folder (the
/// container's tmpfs).
fn game_folder(call: u64) -> io::Result<PathBuf> {
    let tag = data_encoding::HEXLOWER.encode(&crypto::random::<8>());
    let folder = std::env::temp_dir().join(format!("anie-bbs-mines-{call}-{tag}"));
    // Its owner's alone, where there are owners.
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&folder)?;
    Ok(folder)
}

impl Session<'_> {
    pub fn games(&mut self) -> io::Result<()> {
        let items = [
            Item::new('m', "Mines: clear the field, beat the board's best"),
            Item::new('l', "The Mines leaderboard"),
            Item::new('n', "Neko, the cat that chases your pointer"),
            Item::new('b', "Back"),
        ];
        let name = self.bbs.settings.name.clone();
        loop {
            self.doing("In the game room");
            let values = [("board", name.as_str())];
            let Some(chosen) = self.call.menu_titled(Some(&self.bbs.screens.games), &values, "Game room", &items)? else { return Ok(()) };
            match chosen {
                0 => self.mines()?,
                1 => self.leaderboard()?,
                2 => self.neko()?,
                _ => return Ok(()),
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// Neko's door, and back.
    fn neko(&mut self) -> io::Result<()> {
        self.doing("Watching Neko");
        self.call.clear()?;
        let opened = self.call.door(&self.bbs.neko);
        self.after_door()?;
        if let Err(e) = opened {
            eprintln!("anie-bbs: door neko: {e}");
            ui::oops(self.call, "Neko won't come out just now. Please try again later.")?;
            self.pause()?;
        }
        Ok(())
    }

    /// Whatever a door left: plain colors, and the cursor back.
    fn after_door(&mut self) -> io::Result<()> {
        if self.call.hung_up() {
            return Ok(());
        }
        self.call.send("\x1b[0m\x1b[?25h")
    }

    /// A game of Mines, and a member's new bests taken in.
    fn mines(&mut self) -> io::Result<()> {
        self.doing("Playing Mines");
        let member = self.me.as_ref().map(|m| m.id);
        let before: BTreeMap<String, f64> = match member {
            Some(id) => self.db.query("SELECT size, seconds FROM mines_best WHERE member_id = $1", &[&id]).db()?.iter().map(|r| (r.get(0), r.get(1))).collect(),
            None => BTreeMap::new(),
        };
        let played = game_folder(self.id).and_then(|folder| {
            let file = folder.join("flow-mines").join("best.json");
            let ran = self.play_mines(&folder, &file, &before);
            let after = bests(&file);
            let _ = fs::remove_dir_all(&folder);
            ran.map(|_| after)
        });
        self.after_door()?;
        let after = match played {
            Ok(after) => after,
            Err(e) => {
                eprintln!("anie-bbs: door mines: {e}");
                ui::oops(self.call, "Mines isn't here just now. Please try again later.")?;
                self.pause()?;
                return Ok(());
            }
        };
        let better: Vec<(String, f64)> = after.into_iter().filter(|(size, t)| before.get(size).is_none_or(|b| t < b)).collect();
        if better.is_empty() {
            return Ok(());
        }
        let Some(id) = member else {
            if !self.call.hung_up() {
                ui::heading(self.call, "Mines")?;
                for (size, t) in &better {
                    ui::done(self.call, &format!("You cleared {size} in {}.", clock(*t)))?;
                }
                self.call.say(&format!("\n {DIM}Members' times go on the board's leaderboard: apply from the welcome to join.{PLAIN}\n"))?;
                self.pause()?;
            }
            return Ok(());
        };
        // Taken in first, so a member who hangs up straight after a win
        // still has it.
        let mut said = Vec::new();
        for (size, t) in &better {
            self.db
                .execute(
                    "INSERT INTO mines_best (member_id, size, seconds) VALUES ($1, $2, $3) \
                     ON CONFLICT (member_id, size) DO UPDATE SET seconds = EXCLUDED.seconds, at = now() WHERE mines_best.seconds > EXCLUDED.seconds",
                    &[&id, size, t],
                )
                .db()?;
            let r = self
                .db
                .query_one(
                    "SELECT count(*) FILTER (WHERE b.seconds < $2) + 1, count(*) FROM mines_best b JOIN members m ON m.id = b.member_id \
                     WHERE b.size = $1 AND m.status = 'active'",
                    &[size, t],
                )
                .db()?;
            let (rank, of): (i64, i64) = (r.get(0), r.get(1));
            said.push(format!("A new best on {size}: {}, #{rank} of {of} on the leaderboard.", clock(*t)));
        }
        if self.call.hung_up() {
            return Ok(());
        }
        ui::heading(self.call, "Mines")?;
        for line in said {
            ui::done(self.call, &line)?;
        }
        self.pause()?;
        Ok(())
    }

    /// Mines' door, `before` in its file first.
    fn play_mines(&mut self, folder: &Path, file: &Path, before: &BTreeMap<String, f64>) -> io::Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        if !before.is_empty() {
            fs::write(file, serde_json::to_string(before).map_err(io::Error::other)?)?;
        }
        let mut door = Door::new(self.bbs.settings.mines.clone(), Vec::<String>::new());
        door.env.push(("XDG_DATA_HOME".into(), folder.to_string_lossy().into_owned()));
        self.call.clear()?;
        self.call.door(&door).map(|_| ())
    }

    /// The leaderboard: each size's fastest members, and the caller's own
    /// place where it's further down.
    fn leaderboard(&mut self) -> io::Result<()> {
        self.doing("At the leaderboard");
        let me = self.me.as_ref().map(|m| m.id);
        let mut lines = Vec::new();
        for (size, field) in SIZES {
            let rows = self
                .db
                .query(
                    "SELECT m.id, m.handle, b.seconds, to_char(b.at, 'DD Mon YYYY'), rank() OVER (ORDER BY b.seconds) \
                     FROM mines_best b JOIN members m ON m.id = b.member_id \
                     WHERE b.size = $1 AND m.status = 'active' ORDER BY b.seconds, b.at LIMIT 1000",
                    &[&size],
                )
                .db()?;
            lines.push(format!("{CYAN}{size}{PLAIN} {DIM}{field}{PLAIN}"));
            if rows.is_empty() {
                lines.push(format!("  {DIM}No times yet: be the first.{PLAIN}"));
            }
            for (i, r) in rows.iter().enumerate() {
                let (id, handle, t, on, rank): (i64, String, f64, String, i64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
                let mine = Some(id) == me;
                if i >= SHOWN && !mine {
                    continue;
                }
                if i > SHOWN && mine {
                    lines.push(format!("  {DIM}   …{PLAIN}"));
                }
                let medal = match rank {
                    1 => format!("{YELLOW}★{PLAIN}"),
                    2 | 3 => format!("{BOLD}☆{PLAIN}"),
                    _ => " ".into(),
                };
                let (on_mine, you) = if mine { (GREEN, format!("  {GREEN}◂ you{PLAIN}")) } else { (PLAIN, String::new()) };
                lines.push(format!("  {medal} {rank:>3}.  {on_mine}{}{PLAIN} {BOLD}{:>9}{PLAIN}  {DIM}{on}{PLAIN}{you}", ui::pad(&handle, 20), clock(t)));
            }
            lines.push(String::new());
        }
        if me.is_none() {
            lines.push(format!("{DIM}Members' best times go here: apply from the welcome to join.{PLAIN}"));
        }
        if ui::page(self.call, "The Mines leaderboard", &lines)? {
            self.pause()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_shown_to_the_tenth_and_never_rounded_up() {
        assert_eq!(clock(9.25), "0:09.2");
        assert_eq!(clock(65.39), "1:05.3");
        assert_eq!(clock(59.99), "0:59.9");
        assert_eq!(clock(3600.0), "60:00.0");
    }

    #[test]
    fn only_the_sizes_times_are_taken_from_a_mines_file() {
        let dir = std::env::temp_dir().join(format!("anie-bbs-bests-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("best.json");
        fs::write(&file, r#"{"Small": 9.25, "Medium": "fast", "Large": -3, "Custom": 1.0, "Huge": 2.0}"#).unwrap();
        assert_eq!(bests(&file), BTreeMap::from([("Small".to_string(), 9.25)]));
        fs::write(&file, r#"{"Medium": 1e300, "Large": 400.5}"#).unwrap();
        assert_eq!(bests(&file), BTreeMap::from([("Large".to_string(), 400.5)]), "a time past a day is none");
        fs::write(&file, "[1, 2]").unwrap();
        assert!(bests(&file).is_empty(), "not Mines' file");
        assert!(bests(&dir.join("none.json")).is_empty(), "no file");
        let _ = fs::remove_dir_all(&dir);
    }
}
