//! Fixed-size lump records, decoded field by field from little-endian bytes.
//! These mirror the `bspfile.h` structs but keep only the fields used here.

use crate::geom::{Plane, Vec3};

use super::BspError;
use super::lumps::LumpId;

pub struct RawBrush {
    pub first_side: u32,
    pub side_count: u32,
    pub contents: u32,
}

pub struct RawBrushSide {
    pub plane: u16,
}

pub struct RawNode {
    pub children: [i32; 2],
}

pub struct RawLeaf {
    pub first_leaf_brush: u16,
    pub leaf_brush_count: u16,
}

pub struct RawModel {
    pub head_node: i32,
}

pub struct RawFace {
    pub plane: u16,
    pub side: u8,
    pub first_edge: i32,
    pub edge_count: i16,
}

pub struct RawDispInfo {
    pub start: Vec3,
    pub first_vert: i32,
    pub power: i32,
    pub map_face: u16,
}

pub struct RawDispVert {
    pub direction: Vec3,
    pub distance: f32,
}

/// A record type with a fixed size in its lump.
pub trait Record: Sized {
    const LUMP: LumpId;
    fn size(version: u32) -> usize;
    fn decode(bytes: &[u8], version: u32) -> Self;
}

/// Every record in `bytes`, refusing a lump that isn't a whole number of them.
pub fn records<R: Record>(bytes: &[u8], version: u32) -> Result<Vec<R>, BspError> {
    let size = R::size(version);
    if !bytes.len().is_multiple_of(size) {
        return Err(BspError::LumpSize { lump: R::LUMP, len: bytes.len(), record: size });
    }
    Ok(bytes.chunks_exact(size).map(|chunk| R::decode(chunk, version)).collect())
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn i16_at(b: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("records are sized to hold their fields"))
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(b[at..at + 4].try_into().expect("records are sized to hold their fields"))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().expect("records are sized to hold their fields"))
}

fn vec3_at(b: &[u8], at: usize) -> Vec3 {
    Vec3::new(f32_at(b, at), f32_at(b, at + 4), f32_at(b, at + 8))
}

impl Record for Plane {
    const LUMP: LumpId = LumpId::Planes;
    fn size(_: u32) -> usize {
        20
    }
    fn decode(b: &[u8], _: u32) -> Self {
        Plane { normal: vec3_at(b, 0), dist: f32_at(b, 12) }
    }
}

impl Record for Vec3 {
    const LUMP: LumpId = LumpId::Vertexes;
    fn size(_: u32) -> usize {
        12
    }
    fn decode(b: &[u8], _: u32) -> Self {
        vec3_at(b, 0)
    }
}

impl Record for RawBrush {
    const LUMP: LumpId = LumpId::Brushes;
    fn size(_: u32) -> usize {
        12
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawBrush { first_side: u32_at(b, 0), side_count: u32_at(b, 4), contents: u32_at(b, 8) }
    }
}

impl Record for RawBrushSide {
    const LUMP: LumpId = LumpId::BrushSides;
    fn size(_: u32) -> usize {
        8
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawBrushSide { plane: u16_at(b, 0) }
    }
}

impl Record for RawNode {
    const LUMP: LumpId = LumpId::Nodes;
    fn size(_: u32) -> usize {
        32
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawNode { children: [i32_at(b, 4), i32_at(b, 8)] }
    }
}

impl Record for RawLeaf {
    const LUMP: LumpId = LumpId::Leafs;
    /// Version 19 leaves still embed a 24-byte ambient light cube.
    fn size(version: u32) -> usize {
        if version <= 19 { 56 } else { 32 }
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawLeaf { first_leaf_brush: u16_at(b, 24), leaf_brush_count: u16_at(b, 26) }
    }
}

impl Record for RawModel {
    const LUMP: LumpId = LumpId::Models;
    fn size(_: u32) -> usize {
        48
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawModel { head_node: i32_at(b, 36) }
    }
}

impl Record for RawFace {
    const LUMP: LumpId = LumpId::Faces;
    fn size(_: u32) -> usize {
        56
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawFace { plane: u16_at(b, 0), side: b[2], first_edge: i32_at(b, 4), edge_count: i16_at(b, 8) }
    }
}

impl Record for RawDispInfo {
    const LUMP: LumpId = LumpId::DispInfo;
    fn size(_: u32) -> usize {
        176
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawDispInfo { start: vec3_at(b, 0), first_vert: i32_at(b, 12), power: i32_at(b, 20), map_face: u16_at(b, 36) }
    }
}

impl Record for RawDispVert {
    const LUMP: LumpId = LumpId::DispVerts;
    fn size(_: u32) -> usize {
        20
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawDispVert { direction: vec3_at(b, 0), distance: f32_at(b, 12) }
    }
}

/// Edges are pairs of vertex indices.
pub struct RawEdge(pub [u16; 2]);

impl Record for RawEdge {
    const LUMP: LumpId = LumpId::Edges;
    fn size(_: u32) -> usize {
        4
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawEdge([u16_at(b, 0), u16_at(b, 2)])
    }
}

/// A signed edge index; negative means the edge is walked backward.
pub struct RawSurfEdge(pub i32);

impl Record for RawSurfEdge {
    const LUMP: LumpId = LumpId::SurfEdges;
    fn size(_: u32) -> usize {
        4
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawSurfEdge(i32_at(b, 0))
    }
}

/// Leaf-brush indices are u16s.
pub struct RawLeafBrush(pub u16);

impl Record for RawLeafBrush {
    const LUMP: LumpId = LumpId::LeafBrushes;
    fn size(_: u32) -> usize {
        2
    }
    fn decode(b: &[u8], _: u32) -> Self {
        RawLeafBrush(u16_at(b, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_record_is_refused() {
        let result = records::<Plane>(&[0; 30], 20);
        assert!(matches!(result, Err(BspError::LumpSize { lump: LumpId::Planes, len: 30, record: 20 })));
    }

    #[test]
    fn planes_decode_normal_then_distance() {
        let mut bytes = Vec::new();
        for f in [0.0f32, 0.0, 1.0, 64.0] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&2i32.to_le_bytes()); // axis type, ignored
        let planes = records::<Plane>(&bytes, 20).expect("one record");
        assert_eq!(planes, vec![Plane { normal: Vec3::Z, dist: 64.0 }]);
    }

    #[test]
    fn version_19_leaves_are_wider() {
        assert_eq!(records::<RawLeaf>(&[0; 56], 19).expect("one").len(), 1);
        assert!(records::<RawLeaf>(&[0; 56], 20).is_err());
    }
}
