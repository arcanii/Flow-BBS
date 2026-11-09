//! What the cat does, tick by tick, eight ticks a second, as Neko did: it
//! runs to where the caller's pointer is, facing one of eight ways, and
//! once there it sits, washes, scratches, sits, yawns and falls asleep;
//! moving the pointer away startles it awake for a moment before it runs
//! again. Where the pointer's at the window's edge, past where the cat can
//! go, it claws at that edge first. Worked in cells, its place the cell at
//! its top-left, with a cell's height over its width (`aspect`) so it runs
//! as fast up and down as across.

use crate::sprites::{Dir, Edge, Frame};

/// How far it runs a tick, in cells across.
pub const SPEED: f32 = 1.5;

/// Near enough to have arrived, in cells across.
const NEAR: f32 = 1.2;

/// How many ticks each idle state lasts, in order; sleep lasts.
const CLAW: u32 = 16;
const SIT: u32 = 8;
const WASH: u32 = 16;
const SCRATCH: u32 = 16;
const SIT_AGAIN: u32 = 8;
const YAWN: u32 = 8;
/// How long a startled cat stares before it runs.
const STARTLED: u32 = 3;

/// What it's doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Run,
    /// At the edge the pointer's past, clawing at it.
    Claw(Edge),
    Sit,
    Wash,
    Scratch,
    SitAgain,
    Yawn,
    Sleep,
    /// Woken, staring, about to run.
    Startled,
}

/// Something worth a sound or an effect in Flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moment {
    /// It got to the pointer after a run.
    Caught,
    /// It was woken from sleep.
    Woke,
}

/// The cat.
#[derive(Debug, Clone)]
pub struct Cat {
    /// Its top-left cell, in fractions of cells.
    pub x: f32,
    pub y: f32,
    /// Its size in cells.
    pub w: f32,
    pub h: f32,
    pub state: State,
    /// Ticks in this state.
    ticks: u32,
    /// Ticks running, so a short hop doesn't count as a chase.
    ran: u32,
    /// Which way it last ran.
    facing: Dir,
}

/// Which of eight ways (`dx`, `dy`) points, `dy` down the screen and in the
/// same units as `dx`.
pub fn way(dx: f32, dy: f32) -> Dir {
    let angle = (-dy).atan2(dx).to_degrees();
    let sector = ((angle / 45.0).round() as i32).rem_euclid(8);
    [Dir::Right, Dir::UpRight, Dir::Up, Dir::UpLeft, Dir::Left, Dir::DownLeft, Dir::Down, Dir::DownRight][sector as usize]
}

impl Cat {
    /// A cat `w` × `h` cells, sitting with its top-left at (`x`, `y`).
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Cat {
        Cat { x, y, w, h, state: State::Sit, ticks: 0, ran: 0, facing: Dir::Down }
    }

    fn enter(&mut self, state: State) {
        self.state = state;
        self.ticks = 0;
    }

    /// One tick, toward `target` (a cell, or none where the pointer's never
    /// been seen), in a window `cols` × `rows`, a cell `aspect` times as tall
    /// as it's wide. Says what's worth a sound.
    pub fn tick(&mut self, target: Option<(f32, f32)>, cols: f32, rows: f32, aspect: f32) -> Option<Moment> {
        self.ticks += 1;
        // Where it's headed: its middle on the target, held in the window;
        // and the edge it's held at, where the target's past it.
        let (dx, dy, edge) = match target {
            Some((tx, ty)) => {
                let (wantx, wanty) = (tx - self.w / 2.0, ty - self.h / 2.0);
                let gx = wantx.clamp(0.0, (cols - self.w).max(0.0));
                let gy = wanty.clamp(0.0, (rows - self.h).max(0.0));
                let edge = if wantx < gx - 0.5 {
                    Some(Edge::Left)
                } else if wantx > gx + 0.5 {
                    Some(Edge::Right)
                } else if wanty < gy - 0.5 {
                    Some(Edge::Up)
                } else if wanty > gy + 0.5 {
                    Some(Edge::Down)
                } else {
                    None
                };
                (gx - self.x, gy - self.y, edge)
            }
            None => (0.0, 0.0, None),
        };
        // The distance as across: a row is `aspect` cells' widths.
        let far = (dx * dx + (dy * aspect) * (dy * aspect)).sqrt();
        let away = far > NEAR;
        let mut moment = None;
        match self.state {
            State::Run if !away => {
                if self.ran >= 4 {
                    moment = Some(Moment::Caught);
                }
                self.ran = 0;
                self.enter(edge.map_or(State::Sit, State::Claw));
            }
            State::Run => {
                let step = SPEED.min(far);
                self.x += dx / far * step;
                self.y += dy / far * step;
                self.ran += 1;
                self.facing = way(dx, dy * aspect);
            }
            State::Startled if self.ticks >= STARTLED => self.enter(if away { State::Run } else { State::Sit }),
            State::Startled => {}
            _ if away => {
                if self.state == State::Sleep {
                    moment = Some(Moment::Woke);
                }
                self.enter(State::Startled);
            }
            State::Claw(_) if self.ticks >= CLAW => self.enter(State::Sit),
            State::Sit if self.ticks >= SIT => self.enter(State::Wash),
            State::Wash if self.ticks >= WASH => self.enter(State::Scratch),
            State::Scratch if self.ticks >= SCRATCH => self.enter(State::SitAgain),
            State::SitAgain if self.ticks >= SIT_AGAIN => self.enter(State::Yawn),
            State::Yawn if self.ticks >= YAWN => self.enter(State::Sleep),
            _ => {}
        }
        moment
    }

    /// What it looks like now.
    pub fn frame(&self) -> Frame {
        let k = |every: u32| ((self.ticks / every) % 2) as u8;
        match self.state {
            State::Run => Frame::Run(self.facing, (self.ran % 2) as u8),
            State::Claw(edge) => Frame::Claw(edge, k(1)),
            State::Startled => Frame::Awake,
            State::Sit | State::SitAgain => Frame::Still,
            State::Wash => Frame::Wash,
            State::Scratch => Frame::Scratch(k(1)),
            State::Yawn => Frame::Yawn,
            State::Sleep => Frame::Sleep(k(6)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_until(cat: &mut Cat, target: Option<(f32, f32)>, ticks: u32) -> Vec<Option<Moment>> {
        (0..ticks).map(|_| cat.tick(target, 100.0, 30.0, 2.0)).collect()
    }

    #[test]
    fn it_faces_one_of_eight_ways() {
        assert_eq!(way(1.0, 0.0), Dir::Right);
        assert_eq!(way(-1.0, 0.0), Dir::Left);
        assert_eq!(way(0.0, -1.0), Dir::Up);
        assert_eq!(way(0.0, 1.0), Dir::Down);
        assert_eq!(way(1.0, -1.0), Dir::UpRight);
        assert_eq!(way(-1.0, 1.0), Dir::DownLeft);
        assert_eq!(way(-1.0, -1.0), Dir::UpLeft);
        assert_eq!(way(1.0, 1.0), Dir::DownRight);
        assert_eq!(way(10.0, 1.0), Dir::Right, "mostly across is across");
    }

    #[test]
    fn it_runs_to_the_pointer_facing_the_way_it_goes_and_catches_it() {
        let mut cat = Cat::new(80.0, 10.0, 4.0, 2.0);
        cat.enter(State::Run);
        let moments = run_until(&mut cat, Some((12.0, 11.0)), 2);
        assert!(matches!(cat.frame(), Frame::Run(Dir::Left, _)), "{:?}", cat.frame());
        assert!((cat.x - (80.0 - 2.0 * SPEED)).abs() < 0.2, "{}", cat.x);
        assert_eq!(moments, [None, None]);
        let mut caught = false;
        for _ in 0..60 {
            if cat.tick(Some((12.0, 11.0)), 100.0, 30.0, 2.0) == Some(Moment::Caught) {
                caught = true;
                break;
            }
        }
        assert!(caught);
        assert_eq!(cat.state, State::Sit);
        assert!((cat.x - 10.0).abs() < NEAR && (cat.y - 10.0).abs() < NEAR, "its middle on the pointer: ({}, {})", cat.x, cat.y);
    }

    #[test]
    fn it_runs_as_fast_down_as_across_and_faces_up_and_down() {
        let mut cat = Cat::new(10.0, 2.0, 4.0, 2.0);
        cat.enter(State::Run);
        cat.tick(Some((12.0, 25.0)), 100.0, 30.0, 2.0);
        assert!(matches!(cat.frame(), Frame::Run(Dir::Down, _)));
        assert!((cat.y - (2.0 + SPEED / 2.0)).abs() < 0.01, "a row is two cells' widths: {}", cat.y);
        let mut cat = Cat::new(10.0, 20.0, 4.0, 2.0);
        cat.enter(State::Run);
        cat.tick(Some((12.0, 1.0)), 100.0, 30.0, 2.0);
        assert!(matches!(cat.frame(), Frame::Run(Dir::Up, _)));
        let mut cat = Cat::new(10.0, 20.0, 4.0, 2.0);
        cat.enter(State::Run);
        cat.tick(Some((40.0, 6.0)), 100.0, 30.0, 2.0);
        assert!(matches!(cat.frame(), Frame::Run(Dir::UpRight, _)), "{:?}", cat.frame());
    }

    #[test]
    fn once_there_it_sits_washes_scratches_yawns_and_sleeps() {
        let mut cat = Cat::new(10.0, 10.0, 4.0, 2.0);
        let here = Some((12.0, 11.0));
        let mut seen = vec![];
        for _ in 0..80 {
            cat.tick(here, 100.0, 30.0, 2.0);
            if seen.last() != Some(&cat.state) {
                seen.push(cat.state);
            }
        }
        assert_eq!(seen, [State::Sit, State::Wash, State::Scratch, State::SitAgain, State::Yawn, State::Sleep]);
        assert!(matches!(cat.frame(), Frame::Sleep(_)));
    }

    #[test]
    fn at_the_pointer_past_an_edge_it_claws_at_that_edge_first() {
        // Row 30 is the status line's, under the 30 rows the cat has.
        for (target, edge) in [((0.0, 15.0), Edge::Left), ((99.0, 15.0), Edge::Right), ((50.0, 0.0), Edge::Up), ((50.0, 30.0), Edge::Down)] {
            let mut states = vec![];
            let mut cat2 = Cat::new(48.0, 14.0, 4.0, 2.0);
            cat2.enter(State::Run);
            for _ in 0..80 {
                cat2.tick(Some(target), 100.0, 30.0, 2.0);
                if states.last() != Some(&cat2.state) {
                    states.push(cat2.state);
                }
            }
            assert!(states.contains(&State::Claw(edge)), "{target:?}: {states:?}");
            let claw = states.iter().position(|s| *s == State::Claw(edge)).unwrap();
            assert_eq!(states[claw + 1], State::Sit, "then it sits");
        }
    }

    #[test]
    fn moving_the_pointer_away_wakes_it_startled_then_it_runs() {
        let mut cat = Cat::new(10.0, 10.0, 4.0, 2.0);
        run_until(&mut cat, Some((12.0, 11.0)), 80);
        assert_eq!(cat.state, State::Sleep);
        assert_eq!(cat.tick(Some((60.0, 20.0)), 100.0, 30.0, 2.0), Some(Moment::Woke));
        assert_eq!((cat.state, cat.frame()), (State::Startled, Frame::Awake));
        run_until(&mut cat, Some((60.0, 20.0)), STARTLED);
        assert_eq!(cat.state, State::Run);
    }

    #[test]
    fn with_no_pointer_seen_it_stays_put_and_falls_asleep() {
        let mut cat = Cat::new(10.0, 10.0, 4.0, 2.0);
        run_until(&mut cat, None, 80);
        assert_eq!((cat.x, cat.y, cat.state), (10.0, 10.0, State::Sleep));
    }

    #[test]
    fn it_stays_in_the_window_when_the_pointer_is_at_its_edge() {
        let mut cat = Cat::new(50.0, 10.0, 4.0, 2.0);
        cat.enter(State::Run);
        run_until(&mut cat, Some((0.0, 0.0)), 100);
        assert!(cat.x >= 0.0 && cat.y >= 0.0, "({}, {})", cat.x, cat.y);
        run_until(&mut cat, Some((99.0, 29.0)), 100);
        assert!(cat.x + cat.w <= 100.0 + 0.01 && cat.y + cat.h <= 30.0 + 0.01, "({}, {})", cat.x, cat.y);
    }
}
