//! The board's demonstrations, for guests and members: a menu of what Flow
//! draws for a board. The gallery (a picture, a turning model, an effect
//! on the glass and a sound); Newell's teapot, as OpenGL's GLUT draws it;
//! a hypercube turning through four dimensions; a room lit by lamps of its
//! own; and a page of type. The teapot, the hypercube and the room are
//! made in code (`models`), sent once, and turned or played by Flow. In
//! another terminal each says what it would show. Neko, once here, is in
//! the game room now (games.rs).

use std::io;

use flow_bbs::Item;

use crate::session::Session;

impl Session<'_> {
    pub fn demos(&mut self) -> io::Result<()> {
        let items = [
            Item::new('g', "The gallery: a picture, a model, an effect, a sound"),
            Item::new('t', "The teapot, as OpenGL drew it"),
            Item::new('h', "A hypercube, turning through four dimensions"),
            Item::new('l', "Light sources: three lamps going round"),
            Item::new('f', "Typeset fonts"),
            Item::new('b', "Back"),
        ];
        let name = self.bbs.settings.name.clone();
        loop {
            self.doing("In the demonstrations");
            let values = [("board", name.as_str())];
            let Some(chosen) = self.call.menu_titled(Some(&self.bbs.screens.demos), &values, "Demonstrations", &items)? else { return Ok(()) };
            let (screen, doing) = match chosen {
                0 => (&self.bbs.screens.gallery, "In the gallery"),
                1 => (&self.bbs.screens.teapot, "Watching the teapot"),
                2 => (&self.bbs.screens.hypercube, "Watching the hypercube"),
                3 => (&self.bbs.screens.lamps, "Watching the lamps"),
                4 => (&self.bbs.screens.type_page, "Reading the type"),
                _ => return Ok(()),
            };
            self.doing(doing);
            self.call.show(screen, &values)?;
            if !self.pause()? {
                return Ok(());
            }
        }
    }
}
