//! The teapot: Martin Newell's, of 1975 at the University of Utah, as
//! GLUT's `glutSolidTeapot` draws it, bottom and all. Ten bicubic Bézier
//! patches: the rim, body, lid and bottom each made four by reflecting
//! them across x and y, the handle and spout two. Each is worked out at a
//! grid of points with the way it faces there, so it's smooth, then glazed
//! white, as Newell's own Melitta teapot was, and turned on a stand.

use flow_bbs::flow::scene::{cross, dot, normalize, rgb, Material, V3};

use super::glb::{about, Gltf, Node, Path, Surface};

/// Each patch's sixteen control points, four rows of four, by their place
/// in `POINTS`.
const PATCHES: [[usize; 16]; 10] = [
    // The rim.
    [102, 103, 104, 105, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    // The body, upper and lower.
    [12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27],
    [24, 25, 26, 27, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40],
    // The lid, its knob and the rest.
    [96, 96, 96, 96, 97, 98, 99, 100, 101, 101, 101, 101, 0, 1, 2, 3],
    [0, 1, 2, 3, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117],
    // The bottom.
    [118, 118, 118, 118, 124, 122, 119, 121, 123, 126, 125, 120, 40, 39, 38, 37],
    // The handle, upper and lower.
    [41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56],
    [53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 28, 65, 66, 67],
    // The spout, its base and its tip.
    [68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83],
    [80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95],
];

/// How many of `PATCHES`, from the first, are made four by reflection:
/// the rest (the handle and spout) are made two, across y alone.
const ROUND: usize = 6;

/// The control points: x, y, and z up, the teapot standing on z = 0.
const POINTS: [[f32; 3]; 127] = [
    [0.2, 0.0, 2.7], [0.2, -0.112, 2.7], [0.112, -0.2, 2.7], [0.0, -0.2, 2.7],
    [1.3375, 0.0, 2.53125], [1.3375, -0.749, 2.53125], [0.749, -1.3375, 2.53125], [0.0, -1.3375, 2.53125],
    [1.4375, 0.0, 2.53125], [1.4375, -0.805, 2.53125], [0.805, -1.4375, 2.53125], [0.0, -1.4375, 2.53125],
    [1.5, 0.0, 2.4], [1.5, -0.84, 2.4], [0.84, -1.5, 2.4], [0.0, -1.5, 2.4],
    [1.75, 0.0, 1.875], [1.75, -0.98, 1.875], [0.98, -1.75, 1.875], [0.0, -1.75, 1.875],
    [2.0, 0.0, 1.35], [2.0, -1.12, 1.35], [1.12, -2.0, 1.35], [0.0, -2.0, 1.35],
    [2.0, 0.0, 0.9], [2.0, -1.12, 0.9], [1.12, -2.0, 0.9], [0.0, -2.0, 0.9],
    [-2.0, 0.0, 0.9], [2.0, 0.0, 0.45], [2.0, -1.12, 0.45], [1.12, -2.0, 0.45],
    [0.0, -2.0, 0.45], [1.5, 0.0, 0.225], [1.5, -0.84, 0.225], [0.84, -1.5, 0.225],
    [0.0, -1.5, 0.225], [1.5, 0.0, 0.15], [1.5, -0.84, 0.15], [0.84, -1.5, 0.15],
    [0.0, -1.5, 0.15], [-1.6, 0.0, 2.025], [-1.6, -0.3, 2.025], [-1.5, -0.3, 2.25],
    [-1.5, 0.0, 2.25], [-2.3, 0.0, 2.025], [-2.3, -0.3, 2.025], [-2.5, -0.3, 2.25],
    [-2.5, 0.0, 2.25], [-2.7, 0.0, 2.025], [-2.7, -0.3, 2.025], [-3.0, -0.3, 2.25],
    [-3.0, 0.0, 2.25], [-2.7, 0.0, 1.8], [-2.7, -0.3, 1.8], [-3.0, -0.3, 1.8],
    [-3.0, 0.0, 1.8], [-2.7, 0.0, 1.575], [-2.7, -0.3, 1.575], [-3.0, -0.3, 1.35],
    [-3.0, 0.0, 1.35], [-2.5, 0.0, 1.125], [-2.5, -0.3, 1.125], [-2.65, -0.3, 0.9375],
    [-2.65, 0.0, 0.9375], [-2.0, -0.3, 0.9], [-1.9, -0.3, 0.6], [-1.9, 0.0, 0.6],
    [1.7, 0.0, 1.425], [1.7, -0.66, 1.425], [1.7, -0.66, 0.6], [1.7, 0.0, 0.6],
    [2.6, 0.0, 1.425], [2.6, -0.66, 1.425], [3.1, -0.66, 0.825], [3.1, 0.0, 0.825],
    [2.3, 0.0, 2.1], [2.3, -0.25, 2.1], [2.4, -0.25, 2.025], [2.4, 0.0, 2.025],
    [2.7, 0.0, 2.4], [2.7, -0.25, 2.4], [3.3, -0.25, 2.4], [3.3, 0.0, 2.4],
    [2.8, 0.0, 2.475], [2.8, -0.25, 2.475], [3.525, -0.25, 2.49375], [3.525, 0.0, 2.49375],
    [2.9, 0.0, 2.475], [2.9, -0.15, 2.475], [3.45, -0.15, 2.5125], [3.45, 0.0, 2.5125],
    [2.8, 0.0, 2.4], [2.8, -0.15, 2.4], [3.2, -0.15, 2.4], [3.2, 0.0, 2.4],
    [0.0, 0.0, 3.15], [0.8, 0.0, 3.15], [0.8, -0.45, 3.15], [0.45, -0.8, 3.15],
    [0.0, -0.8, 3.15], [0.0, 0.0, 2.85], [1.4, 0.0, 2.4], [1.4, -0.784, 2.4],
    [0.784, -1.4, 2.4], [0.0, -1.4, 2.4], [0.4, 0.0, 2.55], [0.4, -0.224, 2.55],
    [0.224, -0.4, 2.55], [0.0, -0.4, 2.55], [1.3, 0.0, 2.55], [1.3, -0.728, 2.55],
    [0.728, -1.3, 2.55], [0.0, -1.3, 2.55], [1.3, 0.0, 2.4], [1.3, -0.728, 2.4],
    [0.728, -1.3, 2.4], [0.0, -1.3, 2.4], [0.0, 0.0, 0.0], [1.425, -0.798, 0.0],
    [1.5, 0.0, 0.075], [1.425, 0.0, 0.0], [0.798, -1.425, 0.0], [0.0, -1.5, 0.075],
    [0.0, -1.425, 0.0], [1.5, -0.84, 0.075], [0.84, -1.5, 0.075],
];

/// Squares a patch is cut into, each way.
const STEPS: usize = 10;

/// How long it takes to turn once on its stand, in seconds.
const TURN: f32 = 12.0;

/// The four Bernstein polynomials of degree three at `t`, and their
/// slopes.
fn bernstein(t: f32) -> ([f32; 4], [f32; 4]) {
    let s = 1.0 - t;
    ([s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t], [-3.0 * s * s, 3.0 * s * s - 6.0 * t * s, 6.0 * t * s - 3.0 * t * t, 3.0 * t * t])
}

/// Where patch `p` (rows of control points) is at (u, v), and its slopes
/// along u and along v.
fn at(p: &[[V3; 4]; 4], u: f32, v: f32) -> (V3, V3, V3) {
    let ((bu, du), (bv, dv)) = (bernstein(u), bernstein(v));
    let mut out = [[0.0f32; 3]; 3];
    for (j, row) in p.iter().enumerate() {
        for (k, c) in row.iter().enumerate() {
            for (o, w) in out.iter_mut().zip([bu[j] * bv[k], du[j] * bv[k], bu[j] * dv[k]]) {
                for axis in 0..3 {
                    o[axis] += w * c[axis];
                }
            }
        }
    }
    (out[0], out[1], out[2])
}

/// The way patch `p` faces at (u, v): across its slopes, or where those
/// meet in a point (the knob's top, the bottom's middle) just inside it.
fn facing(p: &[[V3; 4]; 4], u: f32, v: f32) -> V3 {
    let (_, du, dv) = at(p, u, v);
    let n = cross(du, dv);
    if dot(n, n) > 1e-12 {
        return n;
    }
    let (_, du, dv) = at(p, u.clamp(1e-3, 1.0 - 1e-3), v.clamp(1e-3, 1.0 - 1e-3));
    cross(du, dv)
}

/// The thirty-two patches, as GLUT makes them of the ten: each as it is
/// and mirrored across y (its columns reversed, to keep it facing out),
/// and the round ones mirrored across x and across both too.
fn patches() -> Vec<[[V3; 4]; 4]> {
    let mut out = Vec::new();
    for (n, patch) in PATCHES.iter().enumerate() {
        let point = |j: usize, k: usize| POINTS[patch[j * 4 + k]];
        let made = |flip_x: bool, flip_y: bool| -> [[V3; 4]; 4] {
            // A reflection across one axis turns the patch inside out, so
            // its columns go the other way; across both, it's only turned.
            let reversed = flip_x != flip_y;
            std::array::from_fn(|j| std::array::from_fn(|k| {
                let [x, y, z] = point(j, if reversed { 3 - k } else { k });
                [if flip_x { -x } else { x }, if flip_y { -y } else { y }, z]
            }))
        };
        out.push(made(false, false));
        out.push(made(false, true));
        if n < ROUND {
            out.push(made(true, false));
            out.push(made(true, true));
        }
    }
    out
}

/// The teapot's surface, standing up (y) with its middle at the origin:
/// GLUT's z up is glTF's y, and its y glTF's -z.
pub fn surface() -> Surface {
    let up = |[x, y, z]: V3| [x, z, -y];
    let mut all = Surface::default();
    for p in patches() {
        all.join(&Surface::grid(STEPS, STEPS, |u, v| {
            let (point, _, _) = at(&p, u, v);
            let [x, y, z] = up(point);
            ([x, y - 1.575, z], normalize(up(facing(&p, u, v))))
        }));
    }
    // Newell's patches all face the same way, in: the body's side
    // furthest along z (away from the handle and the spout) shows which.
    let side = all.at.iter().zip(&all.normal).max_by(|a, b| a.0[2].total_cmp(&b.0[2])).map(|(at, n)| dot(*at, *n));
    if side.is_some_and(|d| d < 0.0) {
        for n in &mut all.normal {
            *n = [-n[0], -n[1], -n[2]];
        }
        for t in all.index.chunks_mut(3) {
            t.swap(1, 2);
        }
    }
    all
}

/// The teapot as a GLB: glazed white porcelain on a stand tipped toward
/// the caller, turning once every `TURN` seconds, lit by Flow's own light.
pub fn glb() -> Vec<u8> {
    let mut g = Gltf::new();
    let porcelain = Material::plain(rgb(0xf3f0e8)).gloss(0.55).coat(1.0, 0.92);
    let mesh = g.mesh(&surface(), porcelain);
    let pot = g.node(Node { mesh: Some(mesh), ..Node::default() });
    g.node(Node { turn: about([1.0, 0.0, 0.0], 0.38), children: vec![pot], ..Node::default() });
    // Eight keys a turn, so each step is the shortest way round.
    let times: Vec<f32> = (0..=8).map(|k| TURN * k as f32 / 8.0).collect();
    let turns: Vec<f32> = (0..=8).flat_map(|k| about([0.0, 1.0, 0.0], std::f32::consts::TAU * k as f32 / 8.0)).collect();
    let keys = g.keys(&times);
    g.animate(pot, Path::Rotation, keys, &turns);
    g.glb()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_teapot_is_thirty_two_patches_standing_up_and_facing_out() {
        assert_eq!(patches().len(), 32);
        let s = surface();
        assert_eq!(s.at.len(), 32 * (STEPS + 1) * (STEPS + 1));
        let (lo, hi) = s.at.iter().fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k]))));
        assert!((lo[0] + 3.0).abs() < 0.01 && (hi[0] - 3.4).abs() < 0.13, "from the handle to the spout's tip: {lo:?} {hi:?}");
        assert!((lo[1] + 1.575).abs() < 1e-4 && (hi[1] - 1.575).abs() < 1e-4, "from the bottom to the knob: {lo:?} {hi:?}");
        // The lid's knob faces up, the bottom down, the body's widest
        // round out.
        let near = |want: V3| s.at.iter().zip(&s.normal).min_by(|a, b| dist(*a.0, want).total_cmp(&dist(*b.0, want))).map(|(_, n)| *n).unwrap();
        assert!(near([0.0, 1.575, 0.0])[1] > 0.9);
        assert!(near([0.0, -1.575, 0.0])[1] < -0.9);
        assert!(near([0.0, -0.2, 2.0])[2] > 0.9 && near([0.0, -0.2, -2.0])[2] < -0.9);
        // Every point of the body's eight patches (after the rim's four)
        // faces out from the teapot's axis.
        let each = (STEPS + 1) * (STEPS + 1);
        for i in 4 * each..12 * each {
            let (at, n) = (s.at[i], s.normal[i]);
            assert!(at[0] * n[0] + at[2] * n[2] > 0.0, "point {i} at {at:?} faces {n:?}");
        }
    }

    fn dist(a: V3, b: V3) -> f32 {
        (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
    }

    #[test]
    fn the_teapot_turns_on_its_stand_by_an_animation_of_its_own() {
        let json = super::super::glb::tests::json_of(&glb());
        let channels = json["animations"][0]["channels"].as_array().unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0]["target"]["path"], "rotation");
        assert_eq!(json["scenes"][0]["nodes"].as_array().unwrap().len(), 1, "the stand alone is a root");
    }
}


