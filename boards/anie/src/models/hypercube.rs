//! The hypercube: a tesseract, the cube of four dimensions, its sixteen
//! corners each one of (±1, ±1, ±1, ±1) and its thirty-two edges joining
//! corners one axis apart. It turns in the plane of its x and w axes, seen
//! in perspective from along w, so its inner cube grows out through the
//! outer one and the outer shrinks in (four times a turn, as each of the
//! cubes along x and w comes round), and the whole turns once about the up
//! axis the while. Each edge is a tube from one corner to the next,
//! colored by the axis it runs along, and each corner a bright ball. Their
//! keys are worked out here, and Flow plays them itself, looped.

use std::f32::consts::{PI, TAU};

use flow_bbs::flow::scene::{cross, dot, normalize, rgb, sub, Material, V3};

use super::glb::{about, ball, then, tube, turned, Gltf, Node, Path};

/// How long a turn takes, in seconds.
const PERIOD: f32 = 32.0;
/// Keys a turn: each 5° of it, which a straight line between them follows
/// closely.
const KEYS: usize = 72;
/// How far along w the eye is from the middle, the corners being 1 away.
const EYE: f32 = 3.0;
/// How big it's drawn.
const SIZE: f32 = 2.4;
/// The edges' tubes' radius and the corners' balls'.
const EDGE: f32 = 0.045;
const CORNER: f32 = 0.09;
/// How far the whole is tipped toward the caller, in radians.
const TIP: f32 = 0.35;
/// Each axis's color: x, y, z and w.
const COLORS: [u32; 4] = [0x45d8ff, 0xff4fc8, 0xffb340, 0x7dff5a];
const UP: V3 = [0.0, 1.0, 0.0];

/// Corner `i` (0 to 15): its bits are which of its axes are +1.
fn corner(i: usize) -> [f32; 4] {
    std::array::from_fn(|k| if (i >> k) & 1 == 1 { 1.0 } else { -1.0 })
}

/// The edges: the corners each joins (the lower first) and the axis it
/// runs along.
fn edges() -> Vec<(usize, usize, usize)> {
    (0..16).flat_map(|i| (0..4).map(move |axis| (i, i ^ (1 << axis), axis))).filter(|(i, j, _)| i < j).collect()
}

/// Where corner `i` shows `t` of the way round (0 to 1): turned in x and w,
/// seen from along w, then turned about the up axis.
fn shown(i: usize, t: f32) -> V3 {
    let [x, y, z, w] = corner(i);
    let (s, c) = (TAU * t).sin_cos();
    let (x, w) = (x * c - w * s, x * s + w * c);
    let k = SIZE / (EYE - w);
    let [x, y, z] = [x * k, y * k, z * k];
    [x * c + z * s, y, z * c - x * s]
}

/// The turn taking `from` to `to` (each of length 1) the shortest way.
fn way(from: V3, to: V3) -> [f32; 4] {
    let d = dot(from, to);
    if d < -0.9999 {
        let other = if from[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        return about(normalize(cross(from, other)), PI);
    }
    let c = cross(from, to);
    let q = [c[0], c[1], c[2], 1.0 + d];
    let len = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    q.map(|v| v / len)
}

/// Each corner's place at each key, and each edge's: where it starts, how
/// it's turned (its tube's y along it) and how long it is.
pub(crate) struct Keys {
    pub corners: Vec<Vec<V3>>,
    pub edges: Vec<Vec<(V3, [f32; 4], f32)>>,
}

pub(crate) fn keys() -> Keys {
    let t = |k: usize| k as f32 / KEYS as f32;
    let corners = (0..16).map(|i| (0..=KEYS).map(|k| shown(i, t(k))).collect()).collect();
    let edges = edges()
        .into_iter()
        .map(|(i, j, _)| {
            // Each key's turn is the last one's carried on the shortest
            // way to the edge's new direction, so a tube never spins about
            // itself between keys.
            let mut turn = [0.0, 0.0, 0.0, 1.0];
            (0..=KEYS)
                .map(|k| {
                    let (from, to) = (shown(i, t(k)), shown(j, t(k)));
                    let along = sub(to, from);
                    let length = dot(along, along).sqrt();
                    turn = then(way(normalize(turned(turn, UP)), normalize(along)), turn);
                    (from, turn, length)
                })
                .collect()
        })
        .collect();
    Keys { corners, edges }
}

/// The hypercube as a GLB.
pub fn glb() -> Vec<u8> {
    let mut g = Gltf::new();
    let shine = |c: u32| {
        let [r, gr, b, _] = rgb(c);
        Material::plain(rgb(c)).gloss(0.5).glow([r * 0.55, gr * 0.55, b * 0.55], 1.0)
    };
    let tube_mesh = g.mesh(&tube(EDGE, 1.0, 10), shine(COLORS[0]));
    let mut by_axis = vec![tube_mesh];
    for &c in &COLORS[1..] {
        by_axis.push(g.mesh_like(tube_mesh, shine(c)));
    }
    let ball_mesh = g.mesh(&ball([0.0; 3], CORNER, 12, 6), Material::plain(rgb(0xffffff)).gloss(0.6).glow([0.85, 0.85, 0.9], 1.0));
    let keys = keys();
    let times: Vec<f32> = (0..=KEYS).map(|k| PERIOD * k as f32 / KEYS as f32).collect();
    let at_keys = g.keys(&times);
    let mut children = Vec::new();
    for (edge, (_, _, axis)) in keys.edges.iter().zip(edges()) {
        let node = g.node(Node { mesh: Some(by_axis[axis]), ..Node::default() });
        g.animate(node, Path::Translation, at_keys, &edge.iter().flat_map(|e| e.0).collect::<Vec<f32>>());
        g.animate(node, Path::Rotation, at_keys, &edge.iter().flat_map(|e| e.1).collect::<Vec<f32>>());
        g.animate(node, Path::Scale, at_keys, &edge.iter().flat_map(|e| [1.0, e.2, 1.0]).collect::<Vec<f32>>());
        children.push(node);
    }
    for corner in &keys.corners {
        let node = g.node(Node { mesh: Some(ball_mesh), ..Node::default() });
        g.animate(node, Path::Translation, at_keys, &corner.iter().flatten().copied().collect::<Vec<f32>>());
        children.push(node);
    }
    g.node(Node { turn: about([1.0, 0.0, 0.0], TIP), children, ..Node::default() });
    g.glb()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tesseract_has_sixteen_corners_and_thirty_two_edges_eight_along_each_axis() {
        let e = edges();
        assert_eq!(e.len(), 32);
        for axis in 0..4 {
            assert_eq!(e.iter().filter(|(_, _, a)| *a == axis).count(), 8);
        }
        assert!(e.iter().all(|&(i, j, axis)| (0..4).all(|k| (corner(i)[k] == corner(j)[k]) == (k != axis))));
    }

    #[test]
    fn each_edge_reaches_from_its_corner_to_the_next_at_every_key_and_the_turn_comes_round() {
        let keys = keys();
        for ((i, j, _), edge) in edges().into_iter().zip(&keys.edges) {
            for (k, &(from, turn, length)) in edge.iter().enumerate() {
                let end = turned(turn, [0.0, length, 0.0]);
                let end = [from[0] + end[0], from[1] + end[1], from[2] + end[2]];
                let (a, b) = (keys.corners[i][k], keys.corners[j][k]);
                assert!((0..3).all(|n| (from[n] - a[n]).abs() < 1e-5 && (end[n] - b[n]).abs() < 1e-3), "edge {i}-{j} at key {k}: {from:?} to {end:?}, not {a:?} to {b:?}");
            }
        }
        for corner in &keys.corners {
            let (first, last) = (corner[0], corner[KEYS]);
            assert!((0..3).all(|n| (first[n] - last[n]).abs() < 1e-4), "a corner comes back where it began, for the loop");
        }
    }

    #[test]
    fn the_inner_cube_comes_out_as_it_turns() {
        // Corner 1 is (1, -1, -1, -1): at the start it's of the inner
        // cube, near the middle; a quarter of the way round, x has turned
        // into w and it's out on the outer one.
        let size = |p: V3| dot(p, p).sqrt();
        assert!(size(shown(1, 0.0)) < size(shown(1, 0.25)) * 0.6);
    }
}
