//! flow-bbsd, Flow's stock BBS server: runs a board from a folder, for
//! sysops who'd rather write files than Rust. It's built on the flow-bbs
//! SDK, as a board in code is.
//!
//! ```text
//! flow-bbsd <folder>
//!
//! board.toml   the name; where it listens, its guest login, its host key;
//!              its menus, and its doors
//! screens/     each screen a text file (the SDK's Screen: colors, names,
//!              sounds, effects, pictures and models)
//! pictures/    what the screens show
//! ```
//!
//! A caller starts at the first menu (`main` unless `first` says). Each of
//! a menu's items shows a screen and comes back, goes to another menu, runs
//! a door and comes back, or hangs up. Everything a board names is checked
//! when it's read, so a mistake is said once, at the start, not to a
//! caller.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use flow_bbs::{Call, Config, Door, Item, Screen};
use serde::Deserialize;

/// `board.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardFile {
    /// The board's name, as callers see it (`{board}` on a screen).
    name: String,
    address: Option<String>,
    guest: Option<String>,
    /// From the folder; made there the first time.
    host_key: Option<PathBuf>,
    /// The menu a caller starts at.
    #[serde(default = "first_menu")]
    first: String,
    #[serde(default)]
    menus: BTreeMap<String, MenuFile>,
    #[serde(default)]
    doors: BTreeMap<String, DoorFile>,
}

fn first_menu() -> String {
    "main".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MenuFile {
    /// The screen above its items, from `screens/`.
    screen: Option<String>,
    items: Vec<ItemFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemFile {
    key: char,
    label: String,
    screen: Option<String>,
    menu: Option<String>,
    door: Option<String>,
    #[serde(default)]
    hang_up: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DoorFile {
    /// The program and its arguments.
    command: Vec<String>,
    /// The folder it runs in, from the board's.
    dir: Option<PathBuf>,
}

/// A board read whole, every screen, menu and door it names found.
#[derive(Debug)]
struct Board {
    name: String,
    first: String,
    screens: BTreeMap<String, Screen>,
    menus: BTreeMap<String, Menu>,
    doors: BTreeMap<String, Door>,
}

#[derive(Debug)]
struct Menu {
    screen: Option<String>,
    items: Vec<Item>,
    does: Vec<Does>,
}

/// What an item does.
#[derive(Debug, Clone, PartialEq)]
enum Does {
    Screen(String),
    Menu(String),
    Door(String),
    HangUp,
}

/// Reads the board in `folder`: how to serve it, and the board.
fn read(folder: &Path) -> Result<(Config, Board), String> {
    let file = folder.join("board.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let b: BoardFile = toml::from_str(&text).map_err(|e| format!("{}: {}", file.display(), e.message()))?;
    let at = |what: String| format!("{}: {what}", file.display());

    let mut menus = b.menus;
    if menus.is_empty() {
        // A board of one screen: welcome, then hang up.
        let items = vec![ItemFile { key: 'q', label: "Hang up".into(), screen: None, menu: None, door: None, hang_up: true }];
        menus.insert("main".into(), MenuFile { screen: Some("welcome".into()), items });
    }
    if !menus.contains_key(&b.first) {
        return Err(at(format!("first names the menu {:?}, which isn't there", b.first)));
    }
    let mut screens = BTreeMap::new();
    let mut read_screen = |name: &str| -> Result<(), String> {
        if !screens.contains_key(name) {
            let path = folder.join("screens").join(format!("{name}.txt"));
            screens.insert(name.to_string(), Screen::read(&path, folder).map_err(|e| e.to_string())?);
        }
        Ok(())
    };
    let mut doors = BTreeMap::new();
    for (name, d) in &b.doors {
        let Some((program, args)) = d.command.split_first() else { return Err(at(format!("door {name}: command is empty"))) };
        let mut door = Door::new(program.clone(), args.to_vec());
        door.dir = Some(folder.join(d.dir.clone().unwrap_or_default()));
        doors.insert(name.clone(), door);
    }
    let mut read_menus = BTreeMap::new();
    for (name, m) in &menus {
        let here = |what: String| at(format!("menu {name}: {what}"));
        if let Some(s) = &m.screen {
            read_screen(s).map_err(here)?;
        }
        if m.items.is_empty() {
            return Err(here("has no items".into()));
        }
        let mut items = Vec::new();
        let mut does = Vec::new();
        for item in &m.items {
            let key = item.key;
            if items.iter().any(|i: &Item| i.key.to_lowercase().eq(key.to_lowercase())) {
                return Err(here(format!("two items take the key {key:?}")));
            }
            let what = match (&item.screen, &item.menu, &item.door, item.hang_up) {
                (Some(s), None, None, false) => {
                    read_screen(s).map_err(here)?;
                    Does::Screen(s.clone())
                }
                (None, Some(m), None, false) if menus.contains_key(m) => Does::Menu(m.clone()),
                (None, Some(m), None, false) => return Err(here(format!("item {key:?} goes to the menu {m:?}, which isn't there"))),
                (None, None, Some(d), false) if doors.contains_key(d) => Does::Door(d.clone()),
                (None, None, Some(d), false) => return Err(here(format!("item {key:?} opens the door {d:?}, which isn't there"))),
                (None, None, None, true) => Does::HangUp,
                _ => return Err(here(format!("item {key:?} must do one thing: a screen, a menu, a door, or hang_up = true"))),
            };
            items.push(Item::new(key, item.label.clone()));
            does.push(what);
        }
        read_menus.insert(name.clone(), Menu { screen: m.screen.clone(), items, does });
    }

    let defaults = Config::default();
    let config = Config {
        address: b.address.unwrap_or(defaults.address.clone()),
        guest: b.guest.unwrap_or(defaults.guest.clone()),
        host_key: folder.join(b.host_key.unwrap_or(defaults.host_key.clone())),
        ..defaults
    };
    Ok((config, Board { name: b.name, first: b.first, screens, menus: read_menus, doors }))
}

impl Board {
    /// One call: the first menu, and wherever its items lead.
    fn call(&self, call: &mut Call) -> io::Result<()> {
        let values = [("board", self.name.as_str())];
        let mut at = self.first.as_str();
        loop {
            let menu = &self.menus[at];
            let screen = menu.screen.as_ref().map(|s| &self.screens[s]);
            let Some(chosen) = call.menu(screen, &values, &menu.items)? else { return Ok(()) };
            match &menu.does[chosen] {
                Does::Screen(s) => {
                    call.show(&self.screens[s], &values)?;
                    if !call.pause("Press any key to go back.")? {
                        return Ok(());
                    }
                }
                Does::Menu(m) => at = m.as_str(),
                Does::Door(d) => {
                    call.send("\x1b[2J\x1b[H")?;
                    if let Err(e) = call.door(&self.doors[d]) {
                        call.say(&format!("\nThat door won't open: {e}\n"))?;
                        eprintln!("flow-bbsd: door {d}: {e}");
                        if !call.pause("Press any key to go back.")? {
                            return Ok(());
                        }
                    }
                    // Whatever the door left, plain colors and the cursor.
                    call.send("\x1b[0m\x1b[?25h")?;
                    if call.hung_up() {
                        return Ok(());
                    }
                }
                Does::HangUp => {
                    call.say("Goodbye.\n")?;
                    return Ok(());
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [folder] = args.as_slice() else {
        eprintln!("usage: flow-bbsd <folder>");
        return ExitCode::from(2);
    };
    let (config, board) = match read(Path::new(folder)) {
        Ok(read) => read,
        Err(e) => {
            eprintln!("flow-bbsd: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("flow-bbsd: {} on {}, its guest login {}", board.name, config.address, config.guest);
    match flow_bbs::serve(config, move |call: &mut Call| board.call(call)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("flow-bbsd: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board folder with `toml` and the screens named.
    fn folder(name: &str, toml: &str, screens: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flow-bbsd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("screens")).unwrap();
        std::fs::write(dir.join("board.toml"), toml).unwrap();
        for (screen, text) in screens {
            std::fs::write(dir.join("screens").join(format!("{screen}.txt")), text).unwrap();
        }
        dir
    }

    #[test]
    fn a_board_of_a_name_and_a_welcome_is_one_screen_then_hang_up() {
        let dir = folder("plain", "name = \"Night Owl\"\n", &[("welcome", "Hi, {user}.")]);
        let (config, board) = read(&dir).unwrap();
        assert_eq!((config.address.as_str(), config.guest.as_str()), ("0.0.0.0:2222", "guest"));
        assert_eq!(config.host_key, dir.join("host_key"), "the host key lives in the board's folder");
        assert_eq!(board.first, "main");
        assert_eq!(board.menus["main"].does, [Does::HangUp]);
        assert!(board.screens.contains_key("welcome"));
    }

    #[test]
    fn menus_lead_to_screens_menus_doors_and_hanging_up() {
        let toml = r#"
            name = "Tea House"
            address = "127.0.0.1:2323"
            [menus.main]
            screen = "welcome"
            items = [
              { key = "a", label = "About", screen = "about" },
              { key = "g", label = "Games", menu = "games" },
              { key = "q", label = "Hang up", hang_up = true },
            ]
            [menus.games]
            items = [{ key = "m", label = "Mines", door = "mines" }, { key = "b", label = "Back", menu = "main" }]
            [doors.mines]
            command = ["flow-mines", "--easy"]
        "#;
        let dir = folder("menus", toml, &[("welcome", "Welcome to {board}."), ("about", "{bold}About{/}")]);
        let (config, board) = read(&dir).unwrap();
        assert_eq!(config.address, "127.0.0.1:2323");
        assert_eq!(board.menus["main"].does, [Does::Screen("about".into()), Does::Menu("games".into()), Does::HangUp]);
        assert_eq!(board.menus["games"].does, [Does::Door("mines".into()), Does::Menu("main".into())]);
        assert_eq!(board.doors["mines"].program, "flow-mines");
        assert_eq!(board.doors["mines"].args, ["--easy"]);
        assert_eq!(board.doors["mines"].dir.as_deref(), Some(dir.as_path()));
    }

    #[test]
    fn every_mistake_is_said_at_the_start_with_its_menu_and_item() {
        let cases = [
            ("name = \"X\"\nadress = \"0.0.0.0:1\"\n", "adress"),
            ("guest = \"g\"\n", "name"),
            ("name = \"X\"\nfirst = \"lobby\"\n", "first names the menu \"lobby\", which isn't there"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\", screen = \"nowhere\" }]\n", "nowhere.txt"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\", menu = \"lobby\" }]\n", "menu main: item 'a' goes to the menu \"lobby\", which isn't there"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\", door = \"rogue\" }]\n", "menu main: item 'a' opens the door \"rogue\", which isn't there"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\", hang_up = true }, { key = \"A\", label = \"B\", hang_up = true }]\n", "two items take the key 'A'"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\" }]\n", "item 'a' must do one thing"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\", hang_up = true, menu = \"main\" }]\n", "item 'a' must do one thing"),
            ("name = \"X\"\n[menus.main]\nitems = []\n", "menu main: has no items"),
            ("name = \"X\"\n[menus.main]\nitems = [{ key = \"a\", label = \"A\", hang_up = true }]\n[doors.d]\ncommand = []\n", "door d: command is empty"),
        ];
        for (n, (toml, says)) in cases.iter().enumerate() {
            let e = read(&folder(&format!("bad{n}"), toml, &[])).err().unwrap();
            assert!(e.contains("board.toml") || e.contains(".txt"), "{e}");
            assert!(e.contains(says), "{toml}\n→ {e}");
        }
        assert!(read(Path::new("/nowhere/at/all")).err().unwrap().starts_with("can't read"));
    }

    #[test]
    fn the_example_board_reads_whole() {
        let (config, board) = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../boards/example")).unwrap();
        assert_eq!(board.name, "Example Board");
        assert_eq!(config.address, "0.0.0.0:2222");
        assert!(board.doors.contains_key("mines"));
        assert_eq!(board.doors["cat"].program, "flow-cat");
    }
}
