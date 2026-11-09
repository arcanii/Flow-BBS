//! The cat's frames: Neko's, drawn by Kenji Gotoh for NekoDA (1989), whose
//! sprites are free to use, modify and redistribute (`sprites/NOTICE.md`).
//! Each is a 32 × 32 PNG in `sprites/`, carried in the program; for Flow
//! they're made bigger here pixel by pixel, so they stay sharp.

use std::io::Cursor;

/// A frame's size in its own pixels.
pub const SIZE: usize = 32;

/// Which way it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    UpRight,
    Right,
    DownRight,
    Down,
    DownLeft,
    Left,
    UpLeft,
}

impl Dir {
    pub const ALL: [Dir; 8] = [Dir::Up, Dir::UpRight, Dir::Right, Dir::DownRight, Dir::Down, Dir::DownLeft, Dir::Left, Dir::UpLeft];

    fn name(self) -> &'static str {
        match self {
            Dir::Up => "up",
            Dir::UpRight => "upright",
            Dir::Right => "right",
            Dir::DownRight => "downright",
            Dir::Down => "down",
            Dir::DownLeft => "downleft",
            Dir::Left => "left",
            Dir::UpLeft => "upleft",
        }
    }
}

/// Which edge of the window it claws at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    Up,
    Right,
    Down,
    Left,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Up, Edge::Right, Edge::Down, Edge::Left];

    fn name(self) -> &'static str {
        match self {
            Edge::Up => "up",
            Edge::Right => "right",
            Edge::Down => "down",
            Edge::Left => "left",
        }
    }
}

/// What the cat looks like at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frame {
    /// Ears up, startled.
    Awake,
    /// Sitting.
    Still,
    Wash,
    Yawn,
    Scratch(u8),
    Sleep(u8),
    Run(Dir, u8),
    Claw(Edge, u8),
}

impl Frame {
    /// Every frame, in the order their pictures are sent.
    pub fn all() -> Vec<Frame> {
        let mut out = vec![Frame::Awake, Frame::Still, Frame::Wash, Frame::Yawn, Frame::Scratch(0), Frame::Scratch(1), Frame::Sleep(0), Frame::Sleep(1)];
        out.extend(Dir::ALL.iter().flat_map(|d| [Frame::Run(*d, 0), Frame::Run(*d, 1)]));
        out.extend(Edge::ALL.iter().flat_map(|e| [Frame::Claw(*e, 0), Frame::Claw(*e, 1)]));
        out
    }

    /// Its place in [`Frame::all`].
    pub fn index(self) -> usize {
        Frame::all().iter().position(|f| *f == self).unwrap_or(0)
    }

    /// Its file's name in `sprites/`, without `.png`.
    pub fn file(self) -> String {
        let n = |k: u8| k % 2 + 1;
        match self {
            Frame::Awake => "awake".into(),
            Frame::Still => "yawn1".into(),
            Frame::Wash => "wash1".into(),
            Frame::Yawn => "yawn2".into(),
            Frame::Scratch(k) => format!("scratch{}", n(k)),
            Frame::Sleep(k) => format!("sleep{}", n(k)),
            Frame::Run(d, k) => format!("{}{}", d.name(), n(k)),
            Frame::Claw(e, k) => format!("{}claw{}", e.name(), n(k)),
        }
    }
}

/// The frames' files, by name.
macro_rules! sprites {
    ($($name:literal)*) => {
        const FILES: &[(&str, &[u8])] = &[$(($name, include_bytes!(concat!("../sprites/", $name, ".png")))),*];
    };
}

sprites! {
    "awake" "yawn1" "wash1" "yawn2" "scratch1" "scratch2" "sleep1" "sleep2"
    "up1" "up2" "upright1" "upright2" "right1" "right2" "downright1" "downright2"
    "down1" "down2" "downleft1" "downleft2" "left1" "left2" "upleft1" "upleft2"
    "upclaw1" "upclaw2" "rightclaw1" "rightclaw2" "downclaw1" "downclaw2" "leftclaw1" "leftclaw2"
}

/// A frame's PNG, as carried.
pub fn png(frame: Frame) -> &'static [u8] {
    let name = frame.file();
    FILES.iter().find(|(n, _)| *n == name).map_or(&[], |(_, bytes)| bytes)
}

/// A PNG's pixels as RGBA, and its size.
fn decode(png: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => bytes.as_chunks::<2>().0.iter().flat_map(|&[v, a]| [v, v, v, a]).collect(),
        png::ColorType::Grayscale => bytes.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width as usize, info.height as usize, rgba))
}

/// A frame as RGBA pixels, each of its pixels `scale` × `scale`, so it's
/// as sharp in a tab as it was on a desktop.
pub fn rgba(frame: Frame, scale: usize) -> Vec<u8> {
    let side = SIZE * scale;
    let mut out = vec![0u8; side * side * 4];
    let Some((w, h, pixels)) = decode(png(frame)) else { return out };
    for y in 0..h.min(SIZE) {
        for x in 0..w.min(SIZE) {
            let p = &pixels[(y * w + x) * 4..(y * w + x) * 4 + 4];
            for dy in 0..scale {
                for dx in 0..scale {
                    let at = ((y * scale + dy) * side + x * scale + dx) * 4;
                    out[at..at + 4].copy_from_slice(p);
                }
            }
        }
    }
    out
}

/// The cat in text, for a terminal without pictures: three rows of at most
/// eight columns, in the sixteen colors.
pub fn text(frame: Frame) -> [&'static str; 3] {
    let legs = |k: u8| if k == 0 { " ^   ^ " } else { "  ^ ^  " };
    match frame {
        Frame::Awake => [" /\\_/\\ ", "( O.O )", " > ^ < "],
        Frame::Still => [" /\\_/\\ ", "( o.o )", " > ^ < "],
        Frame::Wash => [" /\\_/\\ ", "( ^.^ )", " >  ~< "],
        Frame::Yawn => [" /\\_/\\ ", "( -O- )", " > ^ < "],
        Frame::Scratch(0) => [" /\\_/\\~", "( o.< )", " > ^ < "],
        Frame::Scratch(_) => [" /\\_/\\ ", "( o.< )~", " > ^ < "],
        Frame::Sleep(0) => ["     z ", " /\\_/\\ ", "( -.- )"],
        Frame::Sleep(_) => ["    Z  ", " /\\_/\\ ", "( -.- )"],
        Frame::Run(_, k) => [" /\\_/\\ ", "( o.o )", legs(k)],
        Frame::Claw(_, k) => [" /\\_/\\ ", "( >.< )", legs(k)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_has_its_picture_a_32_by_32_png() {
        let all = Frame::all();
        assert_eq!(all.len(), 32);
        assert_eq!(FILES.len(), 32);
        for frame in all {
            let (w, h, pixels) = decode(png(frame)).unwrap_or_else(|| panic!("{frame:?}: {}", frame.file()));
            assert_eq!((w, h, pixels.len()), (SIZE, SIZE, SIZE * SIZE * 4), "{frame:?}");
            assert!(pixels.chunks(4).any(|p| p[3] == 0) && pixels.chunks(4).any(|p| p[3] == 255), "{frame:?} has the cat and see-through around it");
            assert_eq!(Frame::all()[frame.index()], frame);
        }
    }

    #[test]
    fn a_frame_is_made_bigger_pixel_by_pixel() {
        let one = rgba(Frame::Still, 1);
        let three = rgba(Frame::Still, 3);
        assert_eq!(three.len(), 96 * 96 * 4);
        for (x, y) in [(0, 0), (10, 12), (16, 20), (31, 31)] {
            let a = &one[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
            for (dx, dy) in [(0, 0), (2, 2), (1, 0)] {
                let at = ((y * 3 + dy) * 96 + x * 3 + dx) * 4;
                assert_eq!(&three[at..at + 4], a, "({x}, {y}) + ({dx}, {dy})");
            }
        }
    }

    #[test]
    fn the_text_cat_is_three_rows_of_at_most_eight() {
        for frame in Frame::all() {
            for row in text(frame) {
                assert!(row.chars().count() <= 8, "{frame:?}: {row:?}");
            }
        }
    }
}
