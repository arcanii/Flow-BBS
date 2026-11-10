//! Binary glTF made in code, for the demonstrations' models: surfaces with
//! the way they face at every point, so a curve looks curved (flow-sdk's
//! meshes are flat faces, and its scenes are sent anew each frame);
//! materials as flow-sdk writes them; nodes that hold a mesh, a light
//! (KHR_lights_punctual) or both; and one animation of their moves, turns
//! and sizes, which Flow plays itself, looped, so nothing more goes down
//! the call once the file has.

use std::collections::BTreeSet;

use flow_bbs::flow::scene::{add, cross, dot, normalize, sub, Material, V3};
use serde_json::{json, Map, Value};

/// Points, the way the surface faces at each, and its triangles, wound
/// counter-clockwise as seen from the side it faces.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub at: Vec<V3>,
    pub normal: Vec<V3>,
    pub index: Vec<u32>,
}

impl Surface {
    /// A grid of `cols` × `rows` squares, `point` giving where each
    /// corner (u, v from 0 to 1) is and the way it faces; each square two
    /// triangles wound to face as their corners do, and none where a
    /// triangle has no area (at a pole, where a row of corners meets).
    pub fn grid(cols: usize, rows: usize, point: impl Fn(f32, f32) -> (V3, V3)) -> Surface {
        let mut s = Surface::default();
        for j in 0..=rows {
            for i in 0..=cols {
                let (at, normal) = point(i as f32 / cols as f32, j as f32 / rows as f32);
                s.at.push(at);
                s.normal.push(normal);
            }
        }
        let corner = |i: usize, j: usize| (j * (cols + 1) + i) as u32;
        for j in 0..rows {
            for i in 0..cols {
                let (a, b, c, d) = (corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1));
                s.triangle(a, b, c);
                s.triangle(a, c, d);
            }
        }
        s
    }

    /// Triangle `a`, `b`, `c`, wound to face as its corners do; left out
    /// if it has no area.
    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        let [pa, pb, pc] = [a, b, c].map(|i| self.at[i as usize]);
        let face = cross(sub(pb, pa), sub(pc, pa));
        if dot(face, face) < 1e-14 {
            return;
        }
        let facing = [a, b, c].iter().fold([0.0; 3], |n, &i| add(n, self.normal[i as usize]));
        if dot(face, facing) < 0.0 {
            self.index.extend([a, c, b]);
        } else {
            self.index.extend([a, b, c]);
        }
    }

    /// `other` joined on.
    pub fn join(&mut self, other: &Surface) {
        let base = self.at.len() as u32;
        self.at.extend(&other.at);
        self.normal.extend(&other.normal);
        self.index.extend(other.index.iter().map(|i| i + base));
    }

    /// How many triangles it has.
    #[cfg(test)]
    pub fn triangles(&self) -> usize {
        self.index.len() / 3
    }
}

/// A ball of `radius` about `middle`, `around` squares round and `up`
/// from pole to pole.
pub fn ball(middle: V3, radius: f32, around: usize, up: usize) -> Surface {
    Surface::grid(around, up, |u, v| {
        let (yaw, pitch) = (u * std::f32::consts::TAU, (v - 0.5) * std::f32::consts::PI);
        let n = [pitch.cos() * yaw.cos(), pitch.sin(), -pitch.cos() * yaw.sin()];
        (add(middle, [n[0] * radius, n[1] * radius, n[2] * radius]), n)
    })
}

/// A tube of `radius` along y from 0 to `length`, open at its ends,
/// `around` squares round.
pub fn tube(radius: f32, length: f32, around: usize) -> Surface {
    Surface::grid(around, 1, |u, v| {
        let a = u * std::f32::consts::TAU;
        let n = [a.cos(), 0.0, -a.sin()];
        ([n[0] * radius, v * length, n[2] * radius], n)
    })
}

/// A flat disc of `radius` at height `y`, facing `up` (1) or down (-1),
/// `around` squares round and `rings` out from its middle.
pub fn disc(y: f32, radius: f32, up: f32, around: usize, rings: usize) -> Surface {
    Surface::grid(around, rings, |u, v| {
        let a = u * std::f32::consts::TAU;
        ([a.cos() * radius * v, y, -a.sin() * radius * v], [0.0, up, 0.0])
    })
}

/// A box `size` on each side about `middle`, its six faces flat.
pub fn block(middle: V3, size: V3) -> Surface {
    let mut s = Surface::default();
    let half = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
    for axis in 0..3 {
        for side in [-1.0f32, 1.0] {
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            s.join(&Surface::grid(1, 1, |u, v| {
                let mut at = middle;
                at[axis] += side * half[axis];
                at[a] += (u * 2.0 - 1.0) * half[a];
                at[b] += (v * 2.0 - 1.0) * half[b];
                let mut n = [0.0; 3];
                n[axis] = side;
                (at, n)
            }));
        }
    }
    s
}

/// A node of the scene: where it is, how it's turned (a quaternion, x, y,
/// z and w) and sized, and what it holds.
#[derive(Debug, Clone)]
pub struct Node {
    pub mesh: Option<usize>,
    pub light: Option<usize>,
    pub at: V3,
    pub turn: [f32; 4],
    pub size: V3,
    pub children: Vec<usize>,
}

impl Default for Node {
    fn default() -> Self {
        Node { mesh: None, light: None, at: [0.0; 3], turn: [0.0, 0.0, 0.0, 1.0], size: [1.0; 3], children: Vec::new() }
    }
}

/// What an animation's channel moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    Translation,
    Rotation,
    Scale,
}

/// The glTF being made: its buffer, and the JSON's lists.
#[derive(Debug, Default)]
pub struct Gltf {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    materials: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    lights: Vec<Value>,
    roots: Vec<usize>,
    samplers: Vec<Value>,
    channels: Vec<Value>,
    used: BTreeSet<&'static str>,
}

/// glTF's numbers for its kinds of data.
const FLOAT: u32 = 5126;
const SHORT_INDEX: u32 = 5123;
const INDEX: u32 = 5125;
const VERTICES: u32 = 34962;
const TRIANGLES: u32 = 34963;

/// A color as shown, 0 to 1, as glTF's linear light (as flow-sdk has it).
fn linear(c: f32) -> f32 {
    c.clamp(0.0, 1.0).powf(2.2)
}

impl Gltf {
    pub fn new() -> Gltf {
        Gltf::default()
    }

    /// `bytes` as a view of the buffer, from a multiple of four.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut view = json!({"buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len()});
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.bin.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    /// `values`, `width` to an element (`kind` saying which), as an
    /// accessor, with their bounds where `bounds` (points and keys' times
    /// must have them).
    fn floats(&mut self, values: &[f32], kind: &str, width: usize, bounds: bool, target: Option<u32>) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes, target);
        let mut accessor = json!({"bufferView": view, "componentType": FLOAT, "count": values.len() / width, "type": kind});
        if bounds {
            let each = |k: usize| values.iter().skip(k).step_by(width).copied();
            let min: Vec<f32> = (0..width).map(|k| each(k).fold(f32::MAX, f32::min)).collect();
            let max: Vec<f32> = (0..width).map(|k| each(k).fold(f32::MIN, f32::max)).collect();
            accessor["min"] = json!(min);
            accessor["max"] = json!(max);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// A material, as flow-sdk writes one: a plain color matte, anything
    /// more with the factors and extensions that say it.
    pub fn material(&mut self, m: Material) -> usize {
        let [r, g, b, alpha] = m.color;
        let mut pbr = json!({"baseColorFactor": [linear(r), linear(g), linear(b), alpha.clamp(0.0, 1.0)], "metallicFactor": m.metal.clamp(0.0, 1.0)});
        if m.gloss > 0.0 {
            pbr["roughnessFactor"] = json!(1.0 - m.gloss.clamp(0.0, 1.0));
        }
        let mut material = json!({"pbrMetallicRoughness": pbr, "alphaMode": if alpha < 1.0 { "BLEND" } else { "OPAQUE" }, "doubleSided": true});
        let mut extensions = Map::new();
        if m.glow.iter().any(|&c| c > 0.0) {
            material["emissiveFactor"] = json!(m.glow.map(linear));
            if m.glow_strength != 1.0 {
                extensions.insert("KHR_materials_emissive_strength".into(), json!({"emissiveStrength": m.glow_strength.max(0.0)}));
            }
        }
        if m.unlit {
            extensions.insert("KHR_materials_unlit".into(), json!({}));
        }
        if m.coat > 0.0 {
            extensions.insert("KHR_materials_clearcoat".into(), json!({"clearcoatFactor": m.coat.clamp(0.0, 1.0), "clearcoatRoughnessFactor": 1.0 - m.coat_gloss.clamp(0.0, 1.0)}));
        }
        for name in ["KHR_materials_emissive_strength", "KHR_materials_unlit", "KHR_materials_clearcoat"] {
            if extensions.contains_key(name) {
                self.used.insert(name);
            }
        }
        if !extensions.is_empty() {
            material["extensions"] = Value::Object(extensions);
        }
        self.materials.push(material);
        self.materials.len() - 1
    }

    /// A mesh of `surface` in `material`.
    pub fn mesh(&mut self, surface: &Surface, material: Material) -> usize {
        let at: Vec<f32> = surface.at.iter().flatten().copied().collect();
        let normal: Vec<f32> = surface.normal.iter().flat_map(|&n| normalize(n)).collect();
        let position = self.floats(&at, "VEC3", 3, true, Some(VERTICES));
        let normal = self.floats(&normal, "VEC3", 3, false, Some(VERTICES));
        let short = surface.at.len() <= usize::from(u16::MAX);
        let bytes: Vec<u8> = if short {
            surface.index.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect()
        } else {
            surface.index.iter().flat_map(|&i| i.to_le_bytes()).collect()
        };
        let view = self.view(&bytes, Some(TRIANGLES));
        self.accessors.push(json!({"bufferView": view, "componentType": if short { SHORT_INDEX } else { INDEX }, "count": surface.index.len(), "type": "SCALAR"}));
        let indices = self.accessors.len() - 1;
        let material = self.material(material);
        self.meshes.push(json!({"primitives": [{"attributes": {"POSITION": position, "NORMAL": normal}, "indices": indices, "material": material}]}));
        self.meshes.len() - 1
    }

    /// A mesh of `mesh`'s surface in another material, sharing its points.
    pub fn mesh_like(&mut self, mesh: usize, material: Material) -> usize {
        let mut copy = self.meshes[mesh].clone();
        let material = self.material(material);
        copy["primitives"][0]["material"] = json!(material);
        self.meshes.push(copy);
        self.meshes.len() - 1
    }

    /// A light shining all round from where its node is, `color` as
    /// shown, `intensity` against the file's other lights (Flow makes the
    /// strongest four together as bright as its own), and reaching `range`.
    pub fn lamp(&mut self, color: [f32; 3], intensity: f32, range: f32) -> usize {
        self.used.insert("KHR_lights_punctual");
        self.lights.push(json!({"type": "point", "color": color.map(linear), "intensity": intensity, "range": range}));
        self.lights.len() - 1
    }

    /// A node; a root of the scene unless another node takes it as a
    /// child.
    pub fn node(&mut self, node: Node) -> usize {
        let mut json = json!({});
        if let Some(mesh) = node.mesh {
            json["mesh"] = json!(mesh);
        }
        if let Some(light) = node.light {
            json["extensions"] = json!({"KHR_lights_punctual": {"light": light}});
        }
        if node.at != [0.0; 3] {
            json["translation"] = json!(node.at);
        }
        if node.turn != [0.0, 0.0, 0.0, 1.0] {
            json["rotation"] = json!(node.turn);
        }
        if node.size != [1.0; 3] {
            json["scale"] = json!(node.size);
        }
        if !node.children.is_empty() {
            self.roots.retain(|r| !node.children.contains(r));
            json["children"] = json!(node.children);
        }
        self.nodes.push(json);
        self.roots.push(self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// The times of an animation's keys, in seconds, for `animate`.
    pub fn keys(&mut self, times: &[f32]) -> usize {
        self.floats(times, "SCALAR", 1, true, None)
    }

    /// Node `node` moved, turned or sized along `values` at `keys`' times,
    /// in a straight line from each key to the next (turns the shortest
    /// way round). Every channel is of one animation, looped.
    pub fn animate(&mut self, node: usize, path: Path, keys: usize, values: &[f32]) {
        let (name, kind, width) = match path {
            Path::Translation => ("translation", "VEC3", 3),
            Path::Rotation => ("rotation", "VEC4", 4),
            Path::Scale => ("scale", "VEC3", 3),
        };
        let output = self.floats(values, kind, width, false, None);
        self.samplers.push(json!({"input": keys, "output": output, "interpolation": "LINEAR"}));
        self.channels.push(json!({"sampler": self.samplers.len() - 1, "target": {"node": node, "path": name}}));
    }

    /// The file: the JSON chunk, then the buffer's, each padded to four.
    pub fn glb(&self) -> Vec<u8> {
        let mut bin = self.bin.clone();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let mut doc = Map::new();
        doc.insert("asset".into(), json!({"version": "2.0", "generator": "anie-bbs"}));
        doc.insert("scene".into(), json!(0));
        doc.insert("scenes".into(), json!([{"nodes": self.roots}]));
        for (name, list) in [("nodes", &self.nodes), ("meshes", &self.meshes), ("materials", &self.materials), ("accessors", &self.accessors), ("bufferViews", &self.views)] {
            if !list.is_empty() {
                doc.insert(name.into(), json!(list));
            }
        }
        doc.insert("buffers".into(), json!([{"byteLength": bin.len()}]));
        if !self.channels.is_empty() {
            doc.insert("animations".into(), json!([{"name": "loop", "channels": self.channels, "samplers": self.samplers}]));
        }
        if !self.lights.is_empty() {
            doc.insert("extensions".into(), json!({"KHR_lights_punctual": {"lights": self.lights}}));
        }
        if !self.used.is_empty() {
            doc.insert("extensionsUsed".into(), json!(self.used));
        }
        let mut text = serde_json::to_vec(&Value::Object(doc)).expect("JSON of numbers and names");
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let mut out = Vec::with_capacity(28 + text.len() + bin.len());
        out.extend(b"glTF");
        out.extend(2u32.to_le_bytes());
        out.extend((28 + text.len() as u32 + bin.len() as u32).to_le_bytes());
        out.extend((text.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(text);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(bin);
        out
    }
}

/// The quaternion (x, y, z, w) turning `angle` radians about `axis` (of
/// length 1), the right-hand way.
pub fn about(axis: V3, angle: f32) -> [f32; 4] {
    let (s, c) = (angle * 0.5).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

/// `a` after `b`, as one turn.
pub fn then(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let ([ax, ay, az, aw], [bx, by, bz, bw]) = (a, b);
    [aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz]
}

/// `v` turned by `q`.
pub fn turned(q: [f32; 4], v: V3) -> V3 {
    let u = [q[0], q[1], q[2]];
    let t = cross(u, v);
    let t = [t[0] * 2.0, t[1] * 2.0, t[2] * 2.0];
    add(add(v, [t[0] * q[3], t[1] * q[3], t[2] * q[3]]), cross(u, t))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The JSON chunk of a GLB, read back.
    pub(crate) fn json_of(glb: &[u8]) -> Value {
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len(), "the header says the file's length");
        let n = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        serde_json::from_slice(&glb[20..20 + n]).unwrap()
    }

    #[test]
    fn a_grid_faces_as_its_corners_say_and_leaves_out_what_has_no_area() {
        let b = ball([0.0; 3], 1.0, 8, 4);
        assert_eq!(b.at.len(), 9 * 5);
        assert_eq!(b.triangles(), 8 * 4 * 2 - 8 * 2, "a triangle of each square at the poles has no area");
        for t in b.index.chunks(3) {
            let [a, p, q] = [t[0], t[1], t[2]].map(|i| b.at[i as usize]);
            let middle = [(a[0] + p[0] + q[0]) / 3.0, (a[1] + p[1] + q[1]) / 3.0, (a[2] + p[2] + q[2]) / 3.0];
            assert!(dot(cross(sub(p, a), sub(q, a)), middle) > 0.0, "wound to face out");
        }
    }

    #[test]
    fn a_file_has_its_parts_its_light_and_its_animation_and_says_what_it_uses() {
        let mut g = Gltf::new();
        let mesh = g.mesh(&block([0.0; 3], [1.0; 3]), Material::plain([1.0, 0.5, 0.0, 1.0]).gloss(0.5).coat(1.0, 0.9));
        let glow = g.mesh_like(mesh, Material::plain([1.0; 4]).unlit());
        let light = g.lamp([1.0, 0.0, 0.0], 5.0, 4.0);
        let lamp = g.node(Node { mesh: Some(glow), light: Some(light), at: [1.0, 2.0, 0.0], ..Node::default() });
        let pot = g.node(Node { mesh: Some(mesh), ..Node::default() });
        let room = g.node(Node { children: vec![pot, lamp], turn: about([1.0, 0.0, 0.0], 0.5), ..Node::default() });
        let keys = g.keys(&[0.0, 1.0, 2.0]);
        g.animate(lamp, Path::Translation, keys, &[0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0]);
        let glb = g.glb();
        assert_eq!(glb.len() % 4, 0);
        let json = json_of(&glb);
        assert_eq!(json["scenes"][0]["nodes"], json!([room]), "only the room is a root");
        assert_eq!(json["meshes"].as_array().unwrap().len(), 2);
        assert_eq!(json["meshes"][0]["primitives"][0]["attributes"], json["meshes"][1]["primitives"][0]["attributes"], "the glow shares the points");
        assert_eq!(json["nodes"][lamp]["extensions"]["KHR_lights_punctual"]["light"], json!(0));
        assert_eq!(json["animations"][0]["channels"][0]["target"], json!({"node": lamp, "path": "translation"}));
        assert_eq!(json["accessors"][keys]["max"], json!([2.0]));
        assert_eq!(json["extensionsUsed"], json!(["KHR_lights_punctual", "KHR_materials_clearcoat", "KHR_materials_unlit"]));
        assert_eq!(json["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"].as_f64().map(|r| (r * 100.0).round()), Some(50.0));
        let bin = 20 + u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[bin + 4..bin + 8], b"BIN\0");
        assert_eq!(json["buffers"][0]["byteLength"].as_u64(), Some(u32::from_le_bytes(glb[bin..bin + 4].try_into().unwrap()) as u64));
    }

    #[test]
    fn turns_compose_as_they_turn_points() {
        let (a, b) = (about([0.0, 1.0, 0.0], 0.7), about([1.0, 0.0, 0.0], -1.1));
        let v = [0.3, -0.5, 0.8];
        let (one, two) = (turned(then(a, b), v), turned(a, turned(b, v)));
        assert!((0..3).all(|k| (one[k] - two[k]).abs() < 1e-5), "{one:?} {two:?}");
        let quarter = turned(about([0.0, 0.0, 1.0], std::f32::consts::FRAC_PI_2), [1.0, 0.0, 0.0]);
        assert!((quarter[1] - 1.0).abs() < 1e-6 && quarter[0].abs() < 1e-6, "x turns to y about z: {quarter:?}");
    }
}
