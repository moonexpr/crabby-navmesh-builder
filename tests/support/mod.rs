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
