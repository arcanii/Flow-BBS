//! The game room (experimental), for guests and members: Neko, the cat
//! that chases the pointer; Mines, whose members' best times make a
//! leaderboard they share; and Dungeon, the mainframe original of Zork.
//! Each is a door the door server at `ANIE_BBS_DOORS` runs (the SDK's
//! `RemoteDoor`; `flow-doord`, in a container of its own as a user of its
//! own, with no network, as deploy/compose.yaml has it), so a slip in a
//! game reaches none of the board's secrets, its host key, or the network:
//! `neko` (`flow-cat`, this repository's `doors/cat`), `mines`
//! (`flow-mines`, Flow-Games') and `dungeon` (devshane/zork's C, built as
//! deploy/Dockerfile says). Each game runs in a folder of its own, the
//! server's: a member's Dungeon in one kept for them, the rest in one of
//! the call's, gone when the game ends.
//!
//! Mines keeps a best time for each of its sizes in `flow-mines/best.json`
//! in its folder (`XDG_DATA_HOME`). A member's times from the board are put
//! there first, so the game says what they have to beat and writes a time
//! down only when it's beaten; the server gives back what it wrote when
//! the door ends, and the board takes it in. A guest's times are theirs
//! alone, and go nowhere.
//!
//! Dungeon keeps a game its player SAVEs in `dsave.dat` where it runs, so
//! a member's RESTORE finds it on another call. Dungeon starts at its
//! beginning all the same, so the board asks the server when a member's
//! was saved, and tells them, and to RESTORE it.

use std::collections::BTreeMap;
use std::io;
use std::time::SystemTime;

use flow_bbs::{Ended, Item, RemoteDoor};

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

/// Mines' file of best times, in its folder.
const MINES_BESTS: &str = "flow-mines/best.json";

/// Dungeon's saved game, in its folder.
const DUNGEON_SAVE: &str = "dsave.dat";

/// The best times in a Mines file, for its sizes: nothing where it isn't
/// Mines', and no entry that isn't a time.
pub fn bests(text: &str) -> BTreeMap<String, f64> {
    let Ok(serde_json::Value::Object(kept)) = serde_json::from_str(text) else { return BTreeMap::new() };
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

impl Session<'_> {
    pub fn games(&mut self) -> io::Result<()> {
        let items = [
            Item::new('m', "Mines: clear the field, beat the board's best"),
            Item::new('l', "The Mines leaderboard"),
            Item::new('d', "Dungeon: the mainframe original of Zork (1980)"),
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
                2 => self.dungeon()?,
                3 => self.neko()?,
                _ => return Ok(()),
            }
            if self.call.hung_up() {
                return Ok(());
            }
        }
    }

    /// One of the door server's doors, by name, in a folder of the call's.
    fn door(&self, name: &str) -> RemoteDoor {
        RemoteDoor::new(&self.bbs.settings.doors, name)
    }

    /// Neko's door, and back.
    fn neko(&mut self) -> io::Result<()> {
        self.doing("Watching Neko");
        self.call.clear()?;
        let door = self.door("neko");
        let opened = self.call.open_door(&door).and_then(|open| self.call.run_door(open));
        self.after_door()?;
        if let Err(e) = opened {
            eprintln!("anie-bbs: door neko: {e}");
            ui::oops(self.call, "Neko won't come out just now. Please try again later.")?;
            self.pause()?;
        }
        Ok(())
    }

    /// A game of Dungeon: a member's in a folder kept for them (by their
    /// number), a guest's in one of the call's, gone when it ends.
    fn dungeon(&mut self) -> io::Result<()> {
        self.doing("Playing Dungeon");
        let member = self.me.as_ref().map(|m| m.id);
        let mut door = self.door("dungeon");
        door.keep = member.map(|id| id.to_string());
        door.look.push(DUNGEON_SAVE.into());
        let played = self.call.open_door(&door).and_then(|open| {
            // The game always starts at its beginning, a game kept or not, so
            // a member who has one is told it's there to take up.
            let kept = match member {
                Some(_) => match open.modified(DUNGEON_SAVE) {
                    Some(at) => format!("You have a game saved on {}: type RESTORE to take it up.", self.saved_on(at)?),
                    None => "SAVE keeps your game here for next time, and RESTORE takes it up.".into(),
                },
                None => "A guest's game isn't kept: members can SAVE theirs for next time.".into(),
            };
            self.call.clear()?;
            self.call.say(&format!(" {DIM}Dungeon, from MIT in 1980. HELP and INFO tell you more; QUIT leaves.\n {kept}{PLAIN}\n\n"))?;
            self.call.run_door(open)
        });
        self.after_door()?;
        if let Err(e) = played {
            eprintln!("anie-bbs: door dungeon: {e}");
            ui::oops(self.call, "Dungeon isn't here just now. Please try again later.")?;
            self.pause()?;
        }
        Ok(())
    }

    /// When a game was saved, in the board's time zone as its other dates
    /// are.
    fn saved_on(&mut self, at: SystemTime) -> io::Result<String> {
        let secs = at.duration_since(SystemTime::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let row = self.db.query_one("SELECT to_char(to_timestamp($1), 'DD Mon YYYY \"at\" HH24:MI')", &[&secs]).db()?;
        Ok(row.get(0))
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
        let played = self.play_mines(&before);
        self.after_door()?;
        let after = match played {
            Ok(ended) => ended.taken.get(MINES_BESTS).map(|text| bests(text)).unwrap_or_default(),
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

    /// Mines' door, `before` in its file first, and what's in the file when
    /// it ends given back.
    fn play_mines(&mut self, before: &BTreeMap<String, f64>) -> io::Result<Ended> {
        let mut door = self.door("mines");
        if !before.is_empty() {
            door.put.push((MINES_BESTS.into(), serde_json::to_string(before).map_err(io::Error::other)?));
        }
        door.take.push(MINES_BESTS.into());
        self.call.clear()?;
        let open = self.call.open_door(&door)?;
        self.call.run_door(open)
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
        assert_eq!(bests(r#"{"Small": 9.25, "Medium": "fast", "Large": -3, "Custom": 1.0, "Huge": 2.0}"#), BTreeMap::from([("Small".to_string(), 9.25)]));
        assert_eq!(bests(r#"{"Medium": 1e300, "Large": 400.5}"#), BTreeMap::from([("Large".to_string(), 400.5)]), "a time past a day is none");
        assert!(bests("[1, 2]").is_empty(), "not Mines' file");
        assert!(bests("").is_empty(), "nothing in it");
    }
}
