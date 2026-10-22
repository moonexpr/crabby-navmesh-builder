//! Hiding spots, as `CNavArea::ComputeHidingSpots` finds them: an area's
//! corners with no two-way neighbor along either side, each marked in cover
//! or exposed by the lines of sight around it.

use std::collections::HashMap;
use std::f64::consts::PI;

use rayon::prelude::*;

use crate::core::collision::Collision;
use crate::core::geom::Vec3;
use crate::core::mesh::{AreaId, Attributes, Direction, HidingFlags, HidingSpot, NavArea, NavMesh};

/// `NAV_MESH_DONT_HIDE`.
const DONT_HIDE: Attributes = Attributes::from_bits_const(0x0000_0200);
/// A side shorter than this past the neighbors along it still makes a corner.
const CORNER_SIZE: f32 = 20.0;
/// How far a spot sits in from its corner.
const INSET: f32 = 12.5;
/// Spots closer than this to one already placed in the area are left out.
const COLLISION_RANGE: f32 = 30.0;
/// `HalfHumanHeight`: where the cover test looks from, above the spot.
const HALF_HUMAN_HEIGHT: f32 = 35.5;
const OVERHEAD: f32 = 20.0;
const COVER_RANGE: f32 = 100.0;
/// Rays of the 16 around the spot that must hit something for it to be in cover.
const HALF_COVER: usize = 8;

/// Every area's hiding spots, in area order, with ids counting up from 1
/// across the mesh. `cover` is the world as `MASK_NPCSOLID_BRUSHONLY` meets it.
pub fn hiding_spots(mesh: &NavMesh, cover: &dyn Collision) -> Vec<Vec<HidingSpot>> {
    let index: HashMap<AreaId, usize> = mesh.areas.iter().enumerate().map(|(i, a)| (a.id(), i)).collect();
    let found: Vec<Vec<(Vec3, HidingFlags)>> =
        mesh.areas.par_iter().map(|area| area_spots(area, mesh, &index, cover)).collect();
    let mut next_id = 1;
    found
        .into_iter()
        .map(|spots| {
            spots
                .into_iter()
                .map(|(position, flags)| {
                    let id = next_id;
                    next_id += 1;
                    HidingSpot { id, position, flags }
                })
                .collect()
        })
        .collect()
}

/// Corner indices, as `NavCornerType` numbers them and `NavArea::corners` orders them.
const NORTH_WEST: usize = 0;
const NORTH_EAST: usize = 1;
const SOUTH_EAST: usize = 2;
const SOUTH_WEST: usize = 3;

fn area_spots(
    area: &NavArea,
    mesh: &NavMesh,
    index: &HashMap<AreaId, usize>,
    cover: &dyn Collision,
) -> Vec<(Vec3, HidingFlags)> {
    if area.attributes().contains(Attributes::JUMP) || area.attributes().contains(DONT_HIDE) {
        return Vec::new();
    }
    let (nw, se) = (area.north_west(), area.south_east());
    let mut corner_count = [0; 4];
    for direction in Direction::ALL {
        let (mut lo, mut hi) = (999_999.9f32, -999_999.9f32);
        let along_x = matches!(direction, Direction::North | Direction::South);
        for to in area.connections(direction) {
            let Some(other) = index.get(to).map(|&i| &mesh.areas[i]) else {
                continue;
            };
            // a one-way link is a drop: a discontinuity that may mean cover
            if !other.connections(direction.opposite()).contains(&area.id())
                || other.attributes().contains(Attributes::JUMP)
            {
                continue;
            }
            let (other_lo, other_hi) = if along_x {
                (other.north_west().x, other.south_east().x)
            } else {
                (other.north_west().y, other.south_east().y)
            };
            lo = lo.min(other_lo);
            hi = hi.max(other_hi);
        }
        let (first, second, start, end) = match direction {
            Direction::North => (NORTH_WEST, NORTH_EAST, nw.x, se.x),
            Direction::South => (SOUTH_WEST, SOUTH_EAST, nw.x, se.x),
            Direction::East => (NORTH_EAST, SOUTH_EAST, nw.y, se.y),
            Direction::West => (NORTH_WEST, SOUTH_WEST, nw.y, se.y),
        };
        if lo - start >= CORNER_SIZE {
            corner_count[first] += 1;
        }
        if end - hi >= CORNER_SIZE {
            corner_count[second] += 1;
        }
    }
    let mut spots: Vec<(Vec3, HidingFlags)> = Vec::new();
    for (corner, count) in corner_count.into_iter().enumerate() {
        // walls on both sides make a real corner
        if count != 2 {
            continue;
        }
        let position = position_in_area(area, corner);
        // the engine checks every corner but the first against those already placed
        let crowded = spots.iter().any(|(p, _)| (*p - position).length() < COLLISION_RANGE);
        if corner == NORTH_WEST || !crowded {
            let flags = if in_cover(position, cover) { HidingFlags::IN_COVER } else { HidingFlags::EXPOSED };
            spots.push((position, flags));
        }
    }
    spots
}

/// Valve's `FindPositionInArea`: the corner inset toward the area's middle,
/// pulled further in where a small area cannot hold the inset.
fn position_in_area(area: &NavArea, corner: usize) -> Vec3 {
    let (mx, my) = match corner {
        NORTH_WEST => (1.0, 1.0),
        NORTH_EAST => (-1.0, 1.0),
        SOUTH_WEST => (1.0, -1.0),
        _ => (-1.0, -1.0),
    };
    let at = area.corners()[corner];
    let (half_x, half_y) = (area.size_x() * 0.5, area.size_y() * 0.5);
    [(INSET, INSET), (INSET, half_y), (half_x, INSET), (half_x, half_y), (1.0, 1.0)]
        .into_iter()
        .map(|(dx, dy)| at + Vec3::new(dx * mx, dy * my, 0.0))
        .find(|p| area.contains_xy(p.x, p.y))
        .unwrap_or(at)
}

/// Valve's `IsHidingSpotInCover`.
fn in_cover(spot: Vec3, cover: &dyn Collision) -> bool {
    let from = spot + Vec3::new(0.0, 0.0, HALF_HUMAN_HEIGHT);
    // crouched under something counts as cover
    if cover.trace(Vec3::ZERO, from, from + Vec3::new(0.0, 0.0, OVERHEAD)) < 1.0 {
        return true;
    }
    // the engine steps a float angle and compares it in double precision; this repeats its count
    let step = (PI / 8.0) as f32;
    let mut angle = 0.0f32;
    let mut hits = 0;
    while f64::from(angle) < 2.0 * PI {
        let (sin, cos) = f64::from(angle).sin_cos();
        let to = from + Vec3::new(COVER_RANGE * cos as f32, COVER_RANGE * sin as f32, HALF_HUMAN_HEIGHT);
        if cover.trace(Vec3::ZERO, from, to) < 1.0 {
            hits += 1;
        }
        angle += step;
    }
    hits >= HALF_COVER
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::collision::SolidGrid;
    use crate::core::geom::Aabb;
    use crate::core::mesh::CornerHeights;
    use crate::core::solid::ConvexSolid;

    fn id(n: u32) -> AreaId {
        AreaId::new(n).expect("nonzero")
    }

    fn area(
        n: u32,
        min: (f32, f32),
        max: (f32, f32),
        attributes: Attributes,
        connections: [Vec<AreaId>; 4],
    ) -> NavArea {
        let flat = CornerHeights { north_west: 0.0, north_east: 0.0, south_east: 0.0, south_west: 0.0 };
        NavArea::new(id(n), attributes, min, max, flat, connections).expect("valid")
    }

    fn mesh(areas: Vec<NavArea>) -> NavMesh {
        NavMesh { bsp_size: 0, analyzed: false, areas }
    }

    fn solid(min: (f32, f32, f32), max: (f32, f32, f32)) -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2)))
    }

    /// Only a floor far below: nothing to take cover behind.
    fn open_field() -> SolidGrid {
        SolidGrid::new(vec![solid((-5000.0, -5000.0, -500.0), (5000.0, 5000.0, -480.0))]).expect("solid")
    }

    #[test]
    fn a_lone_area_hides_in_all_four_corners() {
        let spots = hiding_spots(
            &mesh(vec![area(1, (0.0, 0.0), (100.0, 100.0), Attributes::NONE, Default::default())]),
            &open_field(),
        );
        let positions: Vec<(f32, f32)> = spots[0].iter().map(|s| (s.position.x, s.position.y)).collect();
        assert_eq!(positions, vec![(12.5, 12.5), (87.5, 12.5), (87.5, 87.5), (12.5, 87.5)]);
        assert!(spots[0].iter().all(|s| s.flags == HidingFlags::EXPOSED), "nothing to hide behind in an open field");
    }

    #[test]
    fn a_two_way_neighbor_along_a_side_takes_its_corners_away() {
        let west = area(1, (0.0, 0.0), (100.0, 100.0), Attributes::NONE, [vec![], vec![id(2)], vec![], vec![]]);
        let east = area(2, (100.0, 0.0), (200.0, 100.0), Attributes::NONE, [vec![], vec![], vec![], vec![id(1)]]);
        let spots = hiding_spots(&mesh(vec![west, east]), &open_field());
        let west_corners: Vec<f32> = spots[0].iter().map(|s| s.position.x).collect();
        assert_eq!(west_corners, vec![12.5, 12.5], "only the west side's two corners");
    }

    #[test]
    fn a_one_way_drop_counts_as_a_wall() {
        let ledge = area(1, (0.0, 0.0), (100.0, 100.0), Attributes::NONE, [vec![], vec![id(2)], vec![], vec![]]);
        let below = area(2, (100.0, 0.0), (200.0, 100.0), Attributes::NONE, Default::default());
        assert_eq!(hiding_spots(&mesh(vec![ledge, below]), &open_field())[0].len(), 4);
    }

    #[test]
    fn jump_and_dont_hide_areas_have_none() {
        let jump = area(1, (0.0, 0.0), (100.0, 100.0), Attributes::JUMP, Default::default());
        let dont = area(2, (500.0, 0.0), (600.0, 100.0), DONT_HIDE, Default::default());
        let spots = hiding_spots(&mesh(vec![jump, dont]), &open_field());
        assert!(spots.iter().all(Vec::is_empty));
    }

    #[test]
    fn spots_crowding_the_first_are_left_out() {
        let small = area(1, (0.0, 0.0), (40.0, 40.0), Attributes::NONE, Default::default());
        assert_eq!(hiding_spots(&mesh(vec![small]), &open_field())[0].len(), 1);
    }

    #[test]
    fn a_spot_under_a_low_roof_is_in_cover() {
        let world = SolidGrid::new(vec![
            solid((-5000.0, -5000.0, -500.0), (5000.0, 5000.0, -480.0)),
            solid((-50.0, -50.0, 45.0), (150.0, 150.0, 60.0)),
        ])
        .expect("solid");
        let spots = hiding_spots(
            &mesh(vec![area(1, (0.0, 0.0), (100.0, 100.0), Attributes::NONE, Default::default())]),
            &world,
        );
        assert!(spots[0].iter().all(|s| s.flags == HidingFlags::IN_COVER));
    }

    #[test]
    fn ids_count_up_across_the_mesh_in_area_order() {
        let a = area(1, (0.0, 0.0), (100.0, 100.0), Attributes::NONE, Default::default());
        let b = area(2, (500.0, 0.0), (600.0, 100.0), Attributes::NONE, Default::default());
        let ids: Vec<u32> = hiding_spots(&mesh(vec![a, b]), &open_field()).iter().flatten().map(|s| s.id).collect();
        assert_eq!(ids, (1..=8).collect::<Vec<_>>());
    }

    #[test]
    fn the_cover_loop_casts_the_engines_sixteen_rays() {
        let inc = (PI / 8.0) as f32;
        let mut angle = 0.0f32;
        let mut rays = 0;
        while f64::from(angle) < 2.0 * PI {
            rays += 1;
            angle += inc;
        }
        assert_eq!(rays, 16);
    }
}
