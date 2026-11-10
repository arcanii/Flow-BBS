//! Models the board makes in code for its demonstrations: binary glTF
//! (`glb`) that a screen names as `made/…`. Each is sent once when its
//! screen shows, then turned or played by Flow itself: the teapot, the
//! hypercube and the room lit by its own lamps.

mod glb;
mod hypercube;
mod lamps;
mod teapot;

/// The board's made models, by the names its screens give them.
pub fn made() -> Vec<(&'static str, Vec<u8>)> {
    vec![("made/teapot.glb", teapot::glb()), ("made/hypercube.glb", hypercube::glb()), ("made/lamps.glb", lamps::glb())]
}

