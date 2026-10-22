//! Writes small but well-formed BSP files for parser tests. Each model gets
//! one node whose children both lead to one leaf listing the model's brushes.

use crabby_navmesh_builder::geom::{Plane, Vec3};

pub const CONTENTS_SOLID: u32 = 0x1;
pub const CONTENTS_WATER: u32 = 0x20;
pub const CONTENTS_PLAYERCLIP: u32 = 0x1_0000;

struct Displacement {
    corners: [Vec3; 4],
    power: i32,
    offsets: Vec<Vec3>,
}

pub struct BspWriter {
    planes: Vec<Plane>,
    brushes: Vec<(Vec<u16>, u32)>,
    models: Vec<Vec<u16>>,
    entities: String,
    displacements: Vec<Displacement>,
}

impl Default for BspWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl BspWriter {
    pub fn new() -> Self {
        Self {
            planes: vec![Plane { normal: Vec3::Z, dist: 0.0 }],
            brushes: vec![],
            models: vec![vec![]],
            entities: "{\n\"classname\" \"worldspawn\"\n}\n".to_string(),
            displacements: vec![],
        }
    }

    pub fn add_model(&mut self) -> usize {
        self.models.push(vec![]);
        self.models.len() - 1
    }

    pub fn brush(&mut self, model: usize, planes: &[Plane], contents: u32) -> usize {
        let sides = planes
            .iter()
            .map(|p| {
                self.planes.push(*p);
                (self.planes.len() - 1) as u16
            })
            .collect();
        self.brushes.push((sides, contents));
        let index = self.brushes.len() - 1;
        self.models[model].push(index as u16);
        index
    }

    pub fn box_brush(&mut self, model: usize, min: Vec3, max: Vec3, contents: u32) -> usize {
        self.brush(model, &box_planes(min, max), contents)
    }

    pub fn entity(&mut self, pairs: &[(&str, &str)]) {
        self.entities.push_str("{\n");
        for (k, v) in pairs {
            self.entities.push_str(&format!("\"{k}\" \"{v}\"\n"));
        }
        self.entities.push_str("}\n");
    }

    /// A displacement on a face with these corners (in winding order),
    /// starting at the first corner.
    pub fn displacement(&mut self, corners: [Vec3; 4], power: i32, offsets: Vec<Vec3>) {
        self.displacements.push(Displacement { corners, power, offsets });
    }

    pub fn build(&self, version: u32) -> Vec<u8> {
        let mut planes = self.planes.clone();
        let mut lumps: Vec<(usize, Vec<u8>)> = Vec::new();

        let mut sides = Vec::new();
        let mut brushes = Vec::new();
        for (side_planes, contents) in &self.brushes {
            put_i32(&mut brushes, (sides.len() / 8) as i32);
            put_i32(&mut brushes, side_planes.len() as i32);
            put_u32(&mut brushes, *contents);
            for p in side_planes {
                put_u16(&mut sides, *p);
                put_i16(&mut sides, 0);
                put_i16(&mut sides, -1);
                put_i16(&mut sides, 0);
            }
        }

        let leaf_size = if version <= 19 { 56 } else { 32 };
        let (mut nodes, mut leafs, mut leaf_brushes, mut models) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (i, model_brushes) in self.models.iter().enumerate() {
            put_i32(&mut nodes, 0);
            put_i32(&mut nodes, -(i as i32) - 1);
            put_i32(&mut nodes, -(i as i32) - 1);
            nodes.extend_from_slice(&[0; 20]);

            let mut leaf = vec![0u8; leaf_size];
            leaf[24..26].copy_from_slice(&((leaf_brushes.len() / 2) as u16).to_le_bytes());
            leaf[26..28].copy_from_slice(&(model_brushes.len() as u16).to_le_bytes());
            leafs.extend_from_slice(&leaf);
            for b in model_brushes {
                put_u16(&mut leaf_brushes, *b);
            }

            models.extend_from_slice(&[0; 36]);
            put_i32(&mut models, i as i32);
            models.extend_from_slice(&[0; 8]);
        }

        let (mut vertexes, mut edges, mut surf_edges, mut faces, mut infos, mut verts) =
            (Vec::new(), vec![0u8; 4], Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (i, d) in self.displacements.iter().enumerate() {
            let first_vertex = (vertexes.len() / 12) as u16;
            for c in d.corners {
                put_vec(&mut vertexes, c);
            }
            let first_edge = (surf_edges.len() / 4) as i32;
            for k in 0..4u16 {
                let edge = (edges.len() / 4) as i32;
                put_u16(&mut edges, first_vertex + k);
                put_u16(&mut edges, first_vertex + (k + 1) % 4);
                put_i32(&mut surf_edges, edge);
            }
            planes.push(Plane { normal: Vec3::Z, dist: d.corners[0].z });
            let mut face = vec![0u8; 56];
            face[0..2].copy_from_slice(&((planes.len() - 1) as u16).to_le_bytes());
            face[4..8].copy_from_slice(&first_edge.to_le_bytes());
            face[8..10].copy_from_slice(&4i16.to_le_bytes());
            face[12..14].copy_from_slice(&(i as i16).to_le_bytes());
            faces.extend_from_slice(&face);

            let mut info = vec![0u8; 176];
            info[0..12].copy_from_slice(&vec_bytes(d.corners[0]));
            info[12..16].copy_from_slice(&((verts.len() / 20) as i32).to_le_bytes());
            info[20..24].copy_from_slice(&d.power.to_le_bytes());
            info[32..36].copy_from_slice(&1i32.to_le_bytes());
            info[36..38].copy_from_slice(&(i as u16).to_le_bytes());
            infos.extend_from_slice(&info);
            for o in &d.offsets {
                let distance = o.length();
                put_vec(&mut verts, if distance > 0.0 { *o / distance } else { Vec3::ZERO });
                put_f32(&mut verts, distance);
                put_f32(&mut verts, 0.0);
            }
        }

        let mut plane_bytes = Vec::new();
        for p in &planes {
            put_vec(&mut plane_bytes, p.normal);
            put_f32(&mut plane_bytes, p.dist);
            put_i32(&mut plane_bytes, 0);
        }

        let mut entities = self.entities.clone().into_bytes();
        entities.push(0);
        lumps.push((0, entities));
        lumps.push((1, plane_bytes));
        lumps.push((3, vertexes));
        lumps.push((5, nodes));
        lumps.push((7, faces));
        lumps.push((10, leafs));
        lumps.push((12, edges));
        lumps.push((13, surf_edges));
        lumps.push((14, models));
        lumps.push((17, leaf_brushes));
        lumps.push((18, brushes));
        lumps.push((19, sides));
        lumps.push((26, infos));
        lumps.push((33, verts));

        let header_size = 8 + 64 * 16 + 4;
        let mut out = vec![0u8; header_size];
        out[0..4].copy_from_slice(b"VBSP");
        out[4..8].copy_from_slice(&version.to_le_bytes());
        for (index, data) in lumps {
            let entry = 8 + index * 16;
            let offset = out.len() as u32;
            out[entry..entry + 4].copy_from_slice(&offset.to_le_bytes());
            out[entry + 4..entry + 8].copy_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&data);
        }
        out
    }
}

pub fn box_planes(min: Vec3, max: Vec3) -> Vec<Plane> {
    vec![
        Plane { normal: Vec3::X, dist: max.x },
        Plane { normal: -Vec3::X, dist: -min.x },
        Plane { normal: Vec3::Y, dist: max.y },
        Plane { normal: -Vec3::Y, dist: -min.y },
        Plane { normal: Vec3::Z, dist: max.z },
        Plane { normal: -Vec3::Z, dist: -min.z },
    ]
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_i16(out: &mut Vec<u8>, v: i16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn vec_bytes(v: Vec3) -> Vec<u8> {
    let mut out = Vec::new();
    put_vec(&mut out, v);
    out
}
fn put_vec(out: &mut Vec<u8>, v: Vec3) {
    put_f32(out, v.x);
    put_f32(out, v.y);
    put_f32(out, v.z);
}
