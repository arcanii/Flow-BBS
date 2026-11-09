//! flow-bbsd, Flow's stock BBS server: runs a board from a folder, for
//! sysops who'd rather write files than Rust. It's built on the flow-bbs
//! SDK, as a board in code is.
//!
//! ```text
//! flow-bbsd <folder>
//!
//! board.toml   the name, and where it listens, its guest login, its host key
//! screens/     welcome.txt, shown to each caller, {user} their login
//! ```
//!
//! Each caller sees the welcome screen and hangs up with any key. The menu,
//! pictures, models, sounds and doors come next (the design's order of
//! work, step 2).

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use flow_bbs::{Call, Config, Key};
use serde::Deserialize;

/// `board.toml`.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Board {
    /// The board's name, as callers see it.
    name: String,
    address: Option<String>,
    guest: Option<String>,
    /// From the folder; made there the first time.
    host_key: Option<PathBuf>,
}

/// A board as flow-bbsd runs it: its name, and its welcome screen.
struct Folder {
    name: String,
    welcome: String,
}

impl Folder {
    fn call(&self, call: &mut Call) -> io::Result<()> {
        let text = self.welcome.replace("{user}", call.user()).replace("{board}", &self.name);
        call.send("\x1b[2J\x1b[H")?;
        call.say(&text)?;
        call.say("\nPress any key to hang up.\n")?;
        while let Some(key) = call.key()? {
            if key != Key::Resize {
                break;
            }
        }
        call.say("Goodbye.\n")
    }
}

/// Reads the board in `folder`: how to serve it, and what it shows.
fn read(folder: &Path) -> Result<(Config, Folder), String> {
    let file = folder.join("board.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let board: Board = toml::from_str(&text).map_err(|e| format!("{}: {}", file.display(), e.message()))?;
    let screen = folder.join("screens").join("welcome.txt");
    let welcome = match std::fs::read_to_string(&screen) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => format!("Welcome to {}, {{user}}.\n", board.name),
        Err(e) => return Err(format!("can't read {}: {e}", screen.display())),
    };
    let defaults = Config::default();
    let config = Config {
        address: board.address.unwrap_or(defaults.address.clone()),
        guest: board.guest.unwrap_or(defaults.guest.clone()),
        host_key: folder.join(board.host_key.unwrap_or(defaults.host_key.clone())),
        ..defaults
    };
    Ok((config, Folder { name: board.name, welcome }))
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

    fn folder(toml: &str, welcome: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flow-bbsd-{}-{}", std::process::id(), toml.len() + welcome.map_or(0, str::len)));
        std::fs::create_dir_all(dir.join("screens")).unwrap();
        std::fs::write(dir.join("board.toml"), toml).unwrap();
        if let Some(text) = welcome {
            std::fs::write(dir.join("screens").join("welcome.txt"), text).unwrap();
        }
        dir
    }

    #[test]
    fn a_board_needs_only_its_name_and_the_rest_has_defaults() {
        let dir = folder("name = \"Night Owl\"\n", None);
        let (config, board) = read(&dir).unwrap();
        assert_eq!((config.address.as_str(), config.guest.as_str()), ("0.0.0.0:2222", "guest"));
        assert_eq!(config.host_key, dir.join("host_key"), "the host key lives in the board's folder");
        assert_eq!(board.welcome, "Welcome to Night Owl, {user}.\n", "a welcome of its own where there's no screen");
    }

    #[test]
    fn a_board_says_where_it_listens_and_its_welcome_comes_from_its_screen() {
        let dir = folder("name = \"Tea House\"\naddress = \"127.0.0.1:2323\"\nguest = \"visitor\"\nhost_key = \"keys/host\"\n", Some("Hello, {user}.\n"));
        let (config, board) = read(&dir).unwrap();
        assert_eq!((config.address.as_str(), config.guest.as_str()), ("127.0.0.1:2323", "visitor"));
        assert_eq!(config.host_key, dir.join("keys/host"));
        assert_eq!(board.welcome, "Hello, {user}.\n");
    }

    #[test]
    fn a_misspelt_or_missing_setting_is_said_with_its_file() {
        let e = read(&folder("name = \"X\"\nadress = \"0.0.0.0:1\"\n", None)).err().unwrap();
        assert!(e.contains("board.toml") && e.contains("adress"), "{e}");
        let e = read(&folder("guest = \"g\"\n", None)).err().unwrap();
        assert!(e.contains("name"), "{e}");
        assert!(read(Path::new("/nowhere/at/all")).err().unwrap().starts_with("can't read"));
    }

    #[test]
    fn the_example_board_reads() {
        let (config, board) = read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../boards/example")).unwrap();
        assert_eq!(board.name, "Example Board");
        assert_eq!(config.address, "0.0.0.0:2222");
        assert!(board.welcome.contains("{user}"));
    }
}
