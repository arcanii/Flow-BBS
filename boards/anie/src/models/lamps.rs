//! Light sources: a white ball, a block and a pillar on a round floor, lit
//! by three lamps of the file's own (glTF's point lights,
//! KHR_lights_punctual), red, green and blue, going round them at three
//! heights and speeds, each a bright bulb in a soft halo. Where two
//! lamps' light falls together it mixes as light does, to yellow, cyan,
//! magenta or white. Flow lights the room by them in place of its own
//! light, and plays their rounds itself, looped.

use std::f32::consts::TAU;

use flow_bbs::flow::scene::{rgb, Material, V3};

use super::glb::{about, ball, block, disc, tube, Gltf, Node, Path};

/// How long the loop takes, in seconds: each lamp goes round a whole
/// number of times in it.
const PERIOD: f32 = 24.0;
/// Keys a loop: the fastest lamp's each 7.5° of its round.
const KEYS: usize = 96;
/// How far the room is tipped toward the caller, in radians.
const TIP: f32 = 0.55;

/// A lamp: its light's color as shown, how far out and how high it goes
/// round, how far it rises and falls, how many times round a loop (less
/// than none the other way), and where in its round it starts.
struct Lamp {
    color: [f32; 3],
    out: f32,
    high: f32,
    bob: f32,
    rounds: f32,
    start: f32,
}

const LAMPS: [Lamp; 3] = [
    Lamp { color: [1.0, 0.22, 0.18], out: 2.2, high: 1.25, bob: 0.2, rounds: 1.0, start: 0.0 },
    Lamp { color: [0.25, 1.0, 0.3], out: 1.8, high: 1.75, bob: 0.25, rounds: -1.0, start: 1.0 / 3.0 },
    Lamp { color: [0.3, 0.45, 1.0], out: 2.55, high: 0.85, bob: 0.15, rounds: 2.0, start: 2.0 / 3.0 },
];

impl Lamp {
    /// Where it is `t` of the way through the loop (0 to 1).
    fn at(&self, t: f32) -> V3 {
        let a = TAU * (self.start + self.rounds * t);
        let rise = self.bob * (TAU * (3.0 * t + self.start)).sin();
        [self.out * a.cos(), self.high + rise, -self.out * a.sin()]
    }
}

/// The room as a GLB.
pub fn glb() -> Vec<u8> {
    let mut g = Gltf::new();
    let floor = g.mesh(&disc(0.0, 3.3, 1.0, 48, 3), Material::plain(rgb(0xb4b4b4)).gloss(0.25));
    let white = g.mesh(&ball([0.0, 0.85, 0.0], 0.85, 36, 18), Material::plain(rgb(0xf2f2f2)).gloss(0.6));
    let cube = g.mesh(&block([0.0, 0.35, 0.0], [0.7; 3]), Material::plain(rgb(0xd8d2c8)));
    let mut column = tube(0.28, 1.5, 24);
    column.join(&disc(1.5, 0.28, 1.0, 24, 1));
    let pillar = g.mesh(&column, Material::plain(rgb(0xe8e8e8)).gloss(0.3));
    let mut children = vec![
        g.node(Node { mesh: Some(floor), ..Node::default() }),
        g.node(Node { mesh: Some(white), ..Node::default() }),
        g.node(Node { mesh: Some(cube), at: [1.5, 0.0, 0.9], turn: about([0.0, 1.0, 0.0], 0.6), ..Node::default() }),
        g.node(Node { mesh: Some(pillar), at: [-1.45, 0.0, -0.85], ..Node::default() }),
    ];
    let bulb = g.mesh(&ball([0.0; 3], 0.11, 12, 6), Material::plain([1.0; 4]).unlit());
    let glow = g.mesh(&ball([0.0; 3], 0.27, 16, 8), Material::plain([1.0, 1.0, 1.0, 0.2]).unlit());
    let times: Vec<f32> = (0..=KEYS).map(|k| PERIOD * k as f32 / KEYS as f32).collect();
    let keys = g.keys(&times);
    for lamp in &LAMPS {
        let [r, gr, b] = lamp.color;
        // The bulb's core is nearer white than its light, as a hot one is.
        let core = g.mesh_like(bulb, Material::plain([(r + 1.0) * 0.5, (gr + 1.0) * 0.5, (b + 1.0) * 0.5, 1.0]).unlit());
        let halo = g.mesh_like(glow, Material::plain([r, gr, b, 0.2]).unlit());
        let halo = g.node(Node { mesh: Some(halo), ..Node::default() });
        let light = g.lamp(lamp.color, 1.0, 7.0);
        let node = g.node(Node { mesh: Some(core), light: Some(light), children: vec![halo], ..Node::default() });
        let path: Vec<f32> = (0..=KEYS).flat_map(|k| lamp.at(k as f32 / KEYS as f32)).collect();
        g.animate(node, Path::Translation, keys, &path);
        children.push(node);
    }
    g.node(Node { turn: about([1.0, 0.0, 0.0], TIP), children, ..Node::default() });
    g.glb()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_lamps_of_the_files_own_go_round_above_the_floor_and_come_back_for_the_loop() {
        let json = super::super::glb::tests::json_of(&glb());
        assert_eq!(json["extensions"]["KHR_lights_punctual"]["lights"].as_array().unwrap().len(), 3);
        assert_eq!(json["animations"][0]["channels"].as_array().unwrap().len(), 3);
        for lamp in &LAMPS {
            assert_eq!(lamp.rounds.fract(), 0.0, "a whole number of rounds a loop");
            let (first, last) = (lamp.at(0.0), lamp.at(1.0));
            assert!((0..3).all(|k| (first[k] - last[k]).abs() < 1e-4));
            for k in 0..=KEYS {
                let p = lamp.at(k as f32 / KEYS as f32);
                assert!(p[1] > 0.5 && (p[0] * p[0] + p[2] * p[2]).sqrt() < 3.3, "over the floor: {p:?}");
            }
        }
    }
}
