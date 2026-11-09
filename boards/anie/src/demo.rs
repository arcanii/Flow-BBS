//! The board's demonstrations, for guests and members: what a board shows
//! in Flow. Neko, the cat that chases the pointer, is a door (`flow-cat`,
//! this repository's `doors/cat`, on the server's PATH); the gallery is a
//! screen of a picture, a turning model, an effect on the glass and a
//! sound. In another terminal Neko is a cat in text and the gallery says
//! what it would show.

use std::io;

use flow_bbs::Item;

use crate::session::Session;
use crate::ui;

impl Session<'_> {
    pub fn demos(&mut self) -> io::Result<()> {
        let items = [Item::new('n', "Neko, the cat that chases your pointer"), Item::new('g', "The gallery: a picture, a turning crystal, an effect, a sound"), Item::new('b', "Back")];
        let name = self.bbs.settings.name.clone();
        loop {
            self.doing("At the demonstrations");
            let values = [("board", name.as_str())];
            let Some(chosen) = self.call.menu(Some(&self.bbs.screens.demos), &values, &items)? else { return Ok(()) };
            match chosen {
                0 => self.neko()?,
                1 => {
                    self.doing("In the gallery");
                    self.call.show(&self.bbs.screens.gallery, &values)?;
                    self.pause()?;
                }
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
        if let Err(e) = self.call.door(&self.bbs.neko) {
            eprintln!("anie-bbs: door neko: {e}");
            ui::oops(self.call, "Neko won't come out just now. Please try again later.")?;
            self.pause()?;
        }
        // Whatever the door left: plain colors, and the cursor back.
        if !self.call.hung_up() {
            self.call.send("\x1b[0m\x1b[?25h")?;
        }
        Ok(())
    }
}
