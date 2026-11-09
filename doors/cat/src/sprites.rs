//! The cat's frames: drawn as text in `sprites.txt`, a letter a pixel, and
//! made into pictures here, each pixel a square of the size the cell needs.
//! The cat running right is its running left turned over; anything else a
//! frame names is drawn once.

/// The frames, as drawn.
const SPRITES: &str = include_str!("../sprites.txt");

/// A frame's size in its own pixels.
pub const SIZE: usize = 16;

/// What the cat looks like at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frame {
    Sit,
    Yawn,
    Groom(u8),
    Scratch(u8),
    Sleep(u8),
    Left(u8),
    Right(u8),
    Up(u8),
    Down(u8),
}

impl Frame {
    /// Every frame, in the order their pictures are sent.
    pub const ALL: [Frame; 16] = [
        Frame::Sit,
        Frame::Yawn,
        Frame::Groom(0),
        Frame::Groom(1),
        Frame::Scratch(0),
        Frame::Scratch(1),
        Frame::Sleep(0),
        Frame::Sleep(1),
        Frame::Left(0),
        Frame::Left(1),
        Frame::Right(0),
        Frame::Right(1),
        Frame::Up(0),
        Frame::Up(1),
        Frame::Down(0),
        Frame::Down(1),
    ];

    /// Its place in [`Frame::ALL`].
    pub fn index(self) -> usize {
        Frame::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    /// The drawing it's made from, and whether it's turned over.
    fn drawing(self) -> (String, bool) {
        let n = |k: u8| k % 2 + 1;
        match self {
            Frame::Sit => ("sit".into(), false),
            Frame::Yawn => ("yawn".into(), false),
            Frame::Groom(k) => (format!("groom{}", n(k)), false),
            Frame::Scratch(k) => (format!("scratch{}", n(k)), false),
            Frame::Sleep(k) => (format!("sleep{}", n(k)), false),
            Frame::Left(k) => (format!("left{}", n(k)), false),
            Frame::Right(k) => (format!("left{}", n(k)), true),
            Frame::Up(k) => (format!("up{}", n(k)), false),
            Frame::Down(k) => (format!("down{}", n(k)), false),
        }
    }
}

/// A letter's color, none for nothing.
fn color(letter: u8) -> Option<[u8; 3]> {
    Some(match letter {
        b'k' => [42, 26, 16],
        b'o' => [232, 137, 43],
        b'd' => [184, 96, 26],
        b'c' => [246, 227, 196],
        b'g' => [95, 207, 106],
        b'p' => [242, 154, 168],
        b'w' => [255, 255, 255],
        _ => return None,
    })
}

/// The drawings in `sprites.txt`, by name: each 16 rows of 16 letters.
pub fn drawings() -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for line in SPRITES.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line.trim().is_empty() {
            out.extend(current.take());
            continue;
        }
        match &mut current {
            None => current = Some((line.trim().to_string(), Vec::new())),
            Some((_, rows)) => rows.push(line.to_string()),
        }
    }
    out.extend(current);
    out
}

/// A frame as RGBA pixels, each of its pixels `scale` × `scale`.
pub fn rgba(frame: Frame, scale: usize) -> Vec<u8> {
    let (name, flipped) = frame.drawing();
    let drawings = drawings();
    let rows = drawings.iter().find(|(n, _)| *n == name).map(|(_, r)| r.clone()).unwrap_or_default();
    let side = SIZE * scale;
    let mut out = vec![0u8; side * side * 4];
    for (y, row) in rows.iter().enumerate().take(SIZE) {
        for (x, letter) in row.bytes().enumerate().take(SIZE) {
            let Some([r, g, b]) = color(letter) else { continue };
            let x = if flipped { SIZE - 1 - x } else { x };
            for dy in 0..scale {
                for dx in 0..scale {
                    let at = ((y * scale + dy) * side + x * scale + dx) * 4;
                    out[at..at + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
    }
    out
}

/// The cat in text, for a terminal without pictures: three rows, seven
/// columns, in the sixteen colors.
pub fn text(frame: Frame) -> [&'static str; 3] {
    match frame {
        Frame::Sit => [" /\\_/\\ ", "( o.o )", " > ^ < "],
        Frame::Yawn => [" /\\_/\\ ", "( -O- )", " > ^ < "],
        Frame::Groom(_) => [" /\\_/\\ ", "( ^.^ )", " >  ~< "],
        Frame::Scratch(0) => [" /\\_/\\~", "( o.< )", " > ^ < "],
        Frame::Scratch(_) => [" /\\_/\\ ", "( o.< )~", " > ^ < "],
        Frame::Sleep(0) => ["     z ", " /\\_/\\ ", "( -.- )"],
        Frame::Sleep(_) => ["    Z  ", " /\\_/\\ ", "( -.- )"],
        Frame::Left(0) | Frame::Up(0) | Frame::Down(0) => [" /\\_/\\ ", "( o.o )", " ^   ^ "],
        Frame::Left(_) | Frame::Up(_) | Frame::Down(_) => [" /\\_/\\ ", "( o.o )", "  ^ ^  "],
        Frame::Right(0) => [" /\\_/\\ ", "( o.o )", " ^   ^ "],
        Frame::Right(_) => [" /\\_/\\ ", "( o.o )", "  ^ ^  "],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drawing_is_16_by_16_in_the_palettes_letters() {
        let drawings = drawings();
        let names: Vec<&str> = drawings.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["sit", "yawn", "groom1", "groom2", "scratch1", "scratch2", "sleep1", "sleep2", "left1", "left2", "up1", "up2", "down1", "down2"]);
        for (name, rows) in &drawings {
            assert_eq!(rows.len(), SIZE, "{name}");
            for row in rows {
                assert_eq!(row.len(), SIZE, "{name}: {row:?}");
                assert!(row.bytes().all(|b| b == b'.' || color(b).is_some()), "{name}: {row:?}");
            }
        }
    }

    #[test]
    fn every_frame_has_its_drawing() {
        let names: Vec<String> = drawings().into_iter().map(|(n, _)| n).collect();
        for frame in Frame::ALL {
            assert!(names.contains(&frame.drawing().0), "{frame:?}");
            assert_eq!(Frame::ALL[frame.index()], frame);
        }
    }

    #[test]
    fn a_frame_is_its_pixels_scaled_and_running_right_is_running_left_turned_over() {
        let left = rgba(Frame::Left(0), 2);
        let right = rgba(Frame::Right(0), 2);
        assert_eq!(left.len(), 32 * 32 * 4);
        let px = |p: &[u8], x: usize, y: usize| p[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4].to_vec();
        for (x, y) in [(0, 0), (4, 10), (20, 12), (31, 31)] {
            assert_eq!(px(&left, x, y), px(&right, 31 - x, y), "({x}, {y})");
        }
        assert_eq!(px(&left, 0, 0)[3], 0, "nothing is see-through");
        assert_eq!(px(&rgba(Frame::Sit, 1), 2, 1), vec![42, 26, 16, 255], "an ear's outline");
    }

    #[test]
    fn the_text_cat_is_three_rows_of_seven() {
        for frame in Frame::ALL {
            for row in text(frame) {
                assert!(row.chars().count() <= 8, "{frame:?}: {row:?}");
            }
        }
    }
}
