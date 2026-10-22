#![allow(dead_code)]

pub mod bsp_writer;

use crabby_navmesh_builder::core::collision::SolidGrid;
use crabby_navmesh_builder::core::geom::{Aabb, Vec3};
use crabby_navmesh_builder::core::mesh::{Direction, NavArea};
use crabby_navmesh_builder::core::solid::ConvexSolid;

pub fn solid(min: (f32, f32, f32), max: (f32, f32, f32)) -> ConvexSolid {
    ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2)))
}

pub fn world(solids: Vec<ConvexSolid>) -> SolidGrid {
    SolidGrid::new(solids).expect("test worlds are not empty")
}

/// The area holding the point `(x, y)` in xy, nearest in height to `z`.
pub fn area_at(areas: &[NavArea], x: f32, y: f32, z: f32) -> Option<&NavArea> {
    areas
        .iter()
        .filter(|a| {
            let (nw, se) = (a.north_west(), a.south_east());
            nw.x <= x && x <= se.x && nw.y <= y && y <= se.y
        })
        .min_by(|a, b| {
            let da = (a.north_west().z - z).abs();
            let db = (b.north_west().z - z).abs();
            da.total_cmp(&db)
        })
}

pub fn connects(from: &NavArea, to: &NavArea) -> bool {
    Direction::ALL.iter().any(|d| from.connections(*d).contains(&to.id()))
}

/// A studio model header carrying only this hull box, enough for a
/// `SOLID_BBOX` prop.
pub fn mdl_with_hull(min: Vec3, max: Vec3) -> Vec<u8> {
    const HULL_OFFSET: usize = 4 + 4 + 4 + 64 + 4 + 12 + 12;
    let mut bytes = vec![0u8; HULL_OFFSET + 24 + 64];
    bytes[0..4].copy_from_slice(b"IDST");
    bytes[4..8].copy_from_slice(&48i32.to_le_bytes());
    for (i, v) in [min.x, min.y, min.z, max.x, max.y, max.z].iter().enumerate() {
        let at = HULL_OFFSET + i * 4;
        bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    bytes
}
