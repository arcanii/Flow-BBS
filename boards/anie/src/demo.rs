//! The board's demonstrations, for guests and members: the gallery, a
//! screen of what a board shows in Flow (a picture, a turning model, an
//! effect on the glass and a sound). In another terminal it says what it
//! would show. Neko, once here, is in the game room now (games.rs).

use std::io;

use crate::session::Session;

impl Session<'_> {
    pub fn demos(&mut self) -> io::Result<()> {
        self.doing("In the gallery");
        let name = self.bbs.settings.name.clone();
        self.call.show(&self.bbs.screens.gallery, &[("board", name.as_str())])?;
        self.pause()?;
        Ok(())
    }
}
