//! Menus: a screen above, a list of keys under it, and the caller's
//! choice. A board's menus lead to its screens, its other menus and its
//! doors; what each choice does is the board's.

use std::io;

use crate::call::Call;
use crate::key::Key;
use crate::screen::Screen;

/// One of a menu's choices: the key that picks it, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub key: char,
    pub label: String,
}

impl Item {
    pub fn new(key: char, label: impl Into<String>) -> Item {
        Item { key, label: label.into() }
    }
}

impl Call {
    /// Shows `screen` (if any) with the items under it, and waits for one
    /// of their keys, a letter's case aside: the item's index. `None` once
    /// the caller hangs up, or presses Ctrl+C or Ctrl+D, which mean the
    /// same here. A resize shows it all again, to the new size.
    pub fn menu(&mut self, screen: Option<&Screen>, values: &[(&str, &str)], items: &[Item]) -> io::Result<Option<usize>> {
        loop {
            match screen {
                Some(s) => {
                    self.show(s, values)?;
                }
                None => {
                    self.forget()?;
                    self.send("\x1b[2J\x1b[H")?;
                }
            }
            let mut list = String::from("\r\n");
            for item in items {
                list.push_str(&format!("  \x1b[1;36m[{}]\x1b[0m {}\r\n", item.key, item.label));
            }
            list.push_str("\r\n  Your choice: ");
            self.send(&list)?;
            loop {
                match self.key()? {
                    None | Some(Key::Ctrl('c' | 'd')) => return Ok(None),
                    Some(Key::Resize) => break,
                    Some(Key::Char(c)) => {
                        if let Some(i) = items.iter().position(|item| item.key.to_lowercase().eq(c.to_lowercase())) {
                            self.say(&format!("{}\n", items[i].key))?;
                            return Ok(Some(i));
                        }
                    }
                    Some(_) => {}
                }
            }
        }
    }

    /// Says `words` and waits for a key: `false` once the caller hangs up,
    /// or presses Ctrl+C or Ctrl+D.
    pub fn pause(&mut self, words: &str) -> io::Result<bool> {
        self.say(&format!("\n\x1b[2m{words}\x1b[0m"))?;
        loop {
            match self.key()? {
                None | Some(Key::Ctrl('c' | 'd')) => return Ok(false),
                Some(Key::Resize) => {}
                Some(_) => return Ok(true),
            }
        }
    }
}
