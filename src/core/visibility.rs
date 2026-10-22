//! Each area's potentially visible set, as `CNavArea::ComputeVisibilityToMesh`
//! computes it: every pair of areas within view distance, traced both ways
//! from eye height, in parallel. The engine also culls by the BSP's PVS;
//! that is only an optimization, since the traces decide.

use rayon::prelude::*;

use crate::core::collision::Collision;
use crate::core::geom::Vec3;
use crate::core::mesh::{NavArea, NavMesh, Visibility, VisibleArea};

/// `nav_max_view_distance`.
pub const MAX_VIEW_DISTANCE: f32 = 6000.0;
/// `nav_potentially_visible_dot_tolerance`: rays this close to the center
/// ray are taken to see what it sees.
const DOT_TOLERANCE: f32 = 0.98;
/// Eye height above the floor: `0.75 * HumanHeight`.
const EYE: f32 = 0.75 * 71.0;
/// `GenerationStepSize`: the spacing of eye positions across an area.
const STEP: f32 = 25.0;
/// Past this distance, eye positions nearly in line with the center are skipped.
const COLLINEAR_RANGE: f32 = 1000.0;

/// Every area's visible set, indexed like `mesh.areas`, each sorted by
/// area id and including the area itself as completely visible. `sight`
/// is the world as `MASK_BLOCKLOS` meets it.
pub fn visible_sets(mesh: &NavMesh, sight: &dyn Collision) -> Vec<Vec<VisibleArea>> {
    let areas = &mesh.areas;
    let centers: Vec<Vec3> = areas.iter().map(NavArea::center).collect();
    let entries: Vec<(usize, VisibleArea)> = (0..areas.len())
        .into_par_iter()
        .flat_map_iter(|a| {
            let mut found = vec![(a, VisibleArea { area: areas[a].id(), visibility: Visibility::COMPLETELY })];
            for b in a + 1..areas.len() {
                if (centers[a] - centers[b]).length() >= MAX_VIEW_DISTANCE {
                    continue;
                }
                // as ComputeVisToArea pairs them, the area visited first computing first
                let mut of_a_from_b = compute_visibility(&areas[a], &areas[b], sight);
                let mut of_b_from_a = compute_visibility(&areas[b], &areas[a], sight);
                if !of_a_from_b.is_visible() && of_b_from_a.is_visible() {
                    of_a_from_b = Visibility::POTENTIALLY;
                }
                if !of_b_from_a.is_visible() && of_a_from_b.is_visible() {
                    of_b_from_a = Visibility::POTENTIALLY;
                }
                if of_b_from_a.is_visible() {
                    found.push((a, VisibleArea { area: areas[b].id(), visibility: of_b_from_a }));
                }
                if of_a_from_b.is_visible() {
                    found.push((b, VisibleArea { area: areas[a].id(), visibility: of_a_from_b }));
                }
            }
            found
        })
        .collect();
    let mut sets = vec![Vec::new(); areas.len()];
    for (owner, entry) in entries {
        sets[owner].push(entry);
    }
    for set in &mut sets {
        set.sort_by_key(|v| v.area);
    }
    sets
}

fn eye(p: Vec3) -> Vec3 {
    p + Vec3::new(0.0, 0.0, EYE)
}

fn line_clear(sight: &dyn Collision, from: Vec3, to: Vec3) -> bool {
    sight.trace(Vec3::ZERO, from, to) >= 1.0
}

/// Valve's `IsPartiallyVisible`: whether a line from `eye_at` reaches the
/// area's center or a corner at eye height.
fn partially_visible(area: &NavArea, eye_at: Vec3, sight: &dyn Collision) -> bool {
    let center = eye(area.center());
    if line_clear(sight, eye_at, center) {
        return true;
    }
    let to_center = (center - eye_at).normalize_or_zero();
    area.corners().into_iter().map(eye).any(|corner| {
        let in_line = (corner - eye_at).normalize_or_zero().dot(to_center) >= DOT_TOLERANCE;
        // the engine raises the corner by the eye height a second time before tracing to it
        !in_line && line_clear(sight, eye_at, eye(corner))
    })
}

/// Valve's `CNavArea::ComputeVisibility(this = from, area = to)`.
fn compute_visibility(from: &NavArea, to: &NavArea, sight: &dyn Collision) -> Visibility {
    let distance_sq = (to.center() - from.center()).length_squared();
    if distance_sq > MAX_VIEW_DISTANCE * MAX_VIEW_DISTANCE {
        return Visibility::NOT_VISIBLE;
    }

    // first, one hull the size of this area swept toward the other's nearest point
    let center = eye(from.center());
    let corners = from.corners().map(eye);
    let low = corners.iter().map(|c| c.z).fold(f32::INFINITY, f32::min);
    let high = corners.iter().map(|c| c.z).fold(f32::NEG_INFINITY, f32::max) + 0.1;
    let mins = Vec3::new(from.north_west().x, from.north_west().y, low) - center;
    let maxs = Vec3::new(from.south_east().x, from.south_east().y, high) - center;
    let (half, offset) = ((maxs - mins) * 0.5, (maxs + mins) * 0.5);
    let (other_min, other_max) = (to.north_west(), to.south_east());
    let clamp = |p: f32, lo: f32, hi: f32| {
        if p < lo {
            lo
        } else if p > hi {
            hi
        } else {
            p
        }
    };
    let mut target = Vec3::new(
        clamp(center.x, other_min.x, other_max.x),
        clamp(center.y, other_min.y, other_max.y),
        clamp(center.z, other_min.z, other_max.z),
    );
    target.z = to.z_at(target.x, target.y) + EYE;
    let reached = sight.trace(half, center + offset, target + offset);
    let end = center + (target - center) * reached;
    let ends_on_other = end.x > other_min.x && end.x < other_max.x && end.y > other_min.y && end.y < other_max.y;
    if reached >= 1.0 || ends_on_other {
        return Visibility::COMPLETELY;
    }

    // then eye positions across this area, each traced to the other's center and corners
    let (potentially, completely) = (Visibility::POTENTIALLY.bits(), Visibility::COMPLETELY.bits());
    let mut vis = completely;
    let look = |eye_at: Vec3, vis: &mut u8| {
        if partially_visible(to, eye_at, sight) {
            *vis |= potentially;
        } else {
            *vis &= !completely;
        }
    };
    look(center, &mut vis);
    let to_center = (from.center() - to.center()).normalize_or_zero();
    let margin = STEP / 2.0;
    let mut dy = margin;
    while dy <= from.size_y() - margin {
        let mut dx = margin;
        while dx <= from.size_x() - margin {
            if vis == potentially {
                return Visibility::POTENTIALLY;
            }
            let (x, y) = (from.north_west().x + dx, from.north_west().y + dy);
            let eye_at = Vec3::new(x, y, from.z_at(x, y) + EYE);
            let in_line = distance_sq > COLLINEAR_RANGE * COLLINEAR_RANGE
                && (eye_at - center).normalize_or_zero().dot(to_center) >= DOT_TOLERANCE;
            if !in_line {
                look(eye_at, &mut vis);
            }
            dx += STEP;
        }
        dy += STEP;
    }
    Visibility::from_bits(vis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::collision::SolidGrid;
    use crate::core::geom::Aabb;
    use crate::core::mesh::{AreaId, Attributes, CornerHeights};
    use crate::core::solid::ConvexSolid;

    fn id(n: u32) -> AreaId {
        AreaId::new(n).expect("nonzero")
    }

    fn area(n: u32, min: (f32, f32), max: (f32, f32)) -> NavArea {
        let flat = CornerHeights { north_west: 0.0, north_east: 0.0, south_east: 0.0, south_west: 0.0 };
        NavArea::new(id(n), Attributes::NONE, min, max, flat, Default::default()).expect("valid")
    }

    fn mesh(areas: Vec<NavArea>) -> NavMesh {
        NavMesh { bsp_size: 0, analyzed: false, areas }
    }

    fn solid(min: (f32, f32, f32), max: (f32, f32, f32)) -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2)))
    }

    fn world(extra: Vec<ConvexSolid>) -> SolidGrid {
        let mut solids = vec![solid((-8000.0, -8000.0, -32.0), (8000.0, 8000.0, -16.0))];
        solids.extend(extra);
        SolidGrid::new(solids).expect("solid")
    }

    fn sees(sets: &[Vec<VisibleArea>], from: usize, to: u32) -> Option<Visibility> {
        sets[from].iter().find(|v| v.area == id(to)).map(|v| v.visibility)
    }

    #[test]
    fn areas_in_the_open_see_each_other_completely_and_themselves() {
        let sets = visible_sets(
            &mesh(vec![area(1, (0.0, 0.0), (100.0, 100.0)), area(2, (0.0, 1000.0), (100.0, 1100.0))]),
            &world(vec![]),
        );
        assert_eq!(sees(&sets, 0, 1), Some(Visibility::COMPLETELY));
        assert_eq!(sees(&sets, 0, 2), Some(Visibility::COMPLETELY));
        assert_eq!(sees(&sets, 1, 1), Some(Visibility::COMPLETELY));
        assert_eq!(sees(&sets, 1, 1).map(|v| v.bits()), Some(2));
    }

    #[test]
    fn a_tall_wall_hides_one_from_the_other() {
        let wall = solid((-500.0, 500.0, -16.0), (600.0, 520.0, 1000.0));
        let sets = visible_sets(
            &mesh(vec![area(1, (0.0, 0.0), (100.0, 100.0)), area(2, (0.0, 1000.0), (100.0, 1100.0))]),
            &world(vec![wall]),
        );
        assert_eq!(sees(&sets, 0, 2), None);
        assert_eq!(sees(&sets, 1, 1), None);
    }

    #[test]
    fn a_wall_hiding_part_of_the_view_leaves_it_potentially_visible() {
        let wall = solid((0.0, 500.0, -16.0), (60.0, 520.0, 1000.0));
        let sets = visible_sets(
            &mesh(vec![area(1, (0.0, 0.0), (100.0, 100.0)), area(2, (0.0, 1000.0), (100.0, 1100.0))]),
            &world(vec![wall]),
        );
        assert_eq!(sees(&sets, 0, 2), Some(Visibility::POTENTIALLY));
        assert_eq!(sees(&sets, 1, 1), Some(Visibility::POTENTIALLY));
    }

    #[test]
    fn areas_beyond_view_distance_are_not_considered() {
        let sets = visible_sets(
            &mesh(vec![area(1, (0.0, 0.0), (100.0, 100.0)), area(2, (0.0, 7000.0), (100.0, 7100.0))]),
            &world(vec![]),
        );
        assert_eq!(sees(&sets, 0, 2), None);
    }

    #[test]
    fn sets_are_sorted_and_the_same_on_any_thread_count() {
        let areas: Vec<NavArea> =
            (0..12).map(|i| area(i + 1, (i as f32 * 300.0, 0.0), (i as f32 * 300.0 + 100.0, 100.0))).collect();
        let wall = solid((1450.0, -500.0, -16.0), (1470.0, 500.0, 1000.0));
        let run = |threads| {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("pool");
            pool.install(|| visible_sets(&mesh(areas.clone()), &world(vec![wall.clone()])))
        };
        let (one, many) = (run(1), run(6));
        assert_eq!(one, many);
        assert!(one.iter().all(|set| set.windows(2).all(|w| w[0].area < w[1].area)));
    }
}
