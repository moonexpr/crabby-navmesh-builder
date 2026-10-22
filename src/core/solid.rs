//! Convex solids: the only collision primitive the generator traces against.
//! Brushes arrive as one directly; displacement triangles become thin prisms;
//! a static prop's collision model is a set of convex [`Hull`]s, placed.

use std::fmt;

use crate::core::geom::{Aabb, Mat3, Plane, Vec3};

/// Distances within this many units count as touching rather than overlapping.
pub const CONTACT_EPSILON: f32 = 0.01;

/// A normal component at least this large makes a plane axial.
const AXIAL: f32 = 0.9999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxialSide {
    MinX,
    MaxX,
    MinY,
    MaxY,
    MinZ,
    MaxZ,
}

impl fmt::Display for AxialSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            AxialSide::MinX => "-x",
            AxialSide::MaxX => "+x",
            AxialSide::MinY => "-y",
            AxialSide::MaxY => "+y",
            AxialSide::MinZ => "-z",
            AxialSide::MaxZ => "+z",
        };
        f.write_str(name)
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SolidError {
    #[error("convex solid has no {0} axial bounding plane")]
    Unbounded(AxialSide),
    #[error("triangle has no area")]
    DegenerateTriangle,
    #[error("hull triangle {triangle} names point {point}, but the hull has {points}")]
    HullIndex { triangle: usize, point: usize, points: usize },
    #[error("hull encloses no volume")]
    DegenerateHull,
}

/// Whether the points span three dimensions: some four of them make a
/// tetrahedron of real volume.
fn encloses_volume(points: &[Vec3]) -> bool {
    const SPREAD: f32 = 1e-3;
    let Some(&first) = points.first() else {
        return false;
    };
    let far = |score: &dyn Fn(Vec3) -> f32| points.iter().copied().max_by(|a, b| score(*a).total_cmp(&score(*b)));
    let Some(second) = far(&|p| (p - first).length()).filter(|p| (*p - first).length() > SPREAD) else {
        return false;
    };
    let axis = (second - first).normalize();
    let off_line = |p: Vec3| (p - first).cross(axis).length();
    let Some(third) = far(&off_line).filter(|p| off_line(*p) > SPREAD) else {
        return false;
    };
    let normal = (second - first).cross(third - first).normalize();
    points.iter().any(|p| normal.dot(*p - first).abs() > SPREAD)
}

/// A convex piece of a collision model in its own space: points, and the
/// triangles of its surface. One that exists has valid indices and volume.
#[derive(Clone, Debug, PartialEq)]
pub struct Hull {
    points: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
}

impl Hull {
    pub fn new(points: Vec<Vec3>, triangles: Vec<[usize; 3]>) -> Result<Self, SolidError> {
        for (triangle, corners) in triangles.iter().enumerate() {
            if let Some(&point) = corners.iter().find(|&&i| i >= points.len()) {
                return Err(SolidError::HullIndex { triangle, point, points: points.len() });
            }
        }
        if !encloses_volume(&points) {
            return Err(SolidError::DegenerateHull);
        }
        Ok(Self { points, triangles })
    }

    /// The hull of an axial box: eight corners, twelve triangles.
    pub fn axis_box(bounds: Aabb) -> Self {
        let (lo, hi) = (bounds.min(), bounds.max());
        // Corner i takes max on x for bit 0, on y for bit 1, on z for bit 2.
        let points = (0..8)
            .map(|i| {
                Vec3::new(
                    if i & 1 == 0 { lo.x } else { hi.x },
                    if i & 2 == 0 { lo.y } else { hi.y },
                    if i & 4 == 0 { lo.z } else { hi.z },
                )
            })
            .collect();
        let faces = [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]];
        let triangles = faces.iter().flat_map(|[a, b, c, d]| [[*a, *b, *c], [*a, *c, *d]]).collect();
        Self { points, triangles }
    }

    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    pub fn triangles(&self) -> &[[usize; 3]] {
        &self.triangles
    }
}

/// The closed vertical interval a solid occupies on one column, with the
/// normal of the surface at its top (the floor a player would stand on).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub bottom: f32,
    pub top: f32,
    pub top_normal: Vec3,
}

/// The intersection of half-spaces `normal · p < dist`. Always carries its six
/// axial planes, which is what makes box tests exact for axial boxes; Source's
/// compiler emits these as bevel sides on every brush.
#[derive(Clone, Debug, PartialEq)]
pub struct ConvexSolid {
    planes: Vec<Plane>,
    bounds: Aabb,
}

impl ConvexSolid {
    /// Builds a solid from its planes, refusing one that lacks an axial plane
    /// on any side.
    pub fn new(planes: Vec<Plane>) -> Result<Self, SolidError> {
        let mut min = Vec3::splat(f32::NEG_INFINITY);
        let mut max = Vec3::splat(f32::INFINITY);
        for plane in &planes {
            let n = plane.normal;
            for axis in 0..3 {
                if n[axis] >= AXIAL {
                    max[axis] = max[axis].min(plane.dist / n[axis]);
                } else if n[axis] <= -AXIAL {
                    min[axis] = min[axis].max(plane.dist / n[axis]);
                }
            }
        }
        let sides = [
            (min.x, AxialSide::MinX),
            (max.x, AxialSide::MaxX),
            (min.y, AxialSide::MinY),
            (max.y, AxialSide::MaxY),
            (min.z, AxialSide::MinZ),
            (max.z, AxialSide::MaxZ),
        ];
        if let Some((_, side)) = sides.iter().find(|(bound, _)| bound.is_infinite()) {
            return Err(SolidError::Unbounded(*side));
        }
        Ok(Self { planes, bounds: Aabb::from_corners(min, max) })
    }

    pub fn axis_box(bounds: Aabb) -> Self {
        let (min, max) = (bounds.min(), bounds.max());
        let planes = vec![
            Plane { normal: Vec3::X, dist: max.x },
            Plane { normal: -Vec3::X, dist: -min.x },
            Plane { normal: Vec3::Y, dist: max.y },
            Plane { normal: -Vec3::Y, dist: -min.y },
            Plane { normal: Vec3::Z, dist: max.z },
            Plane { normal: -Vec3::Z, dist: -min.z },
        ];
        Self { planes, bounds }
    }

    /// A prism of the given thickness behind the triangle's face. The face's
    /// outward normal is the triangle normal oriented to agree with `up`.
    pub fn from_triangle(corners: [Vec3; 3], up: Vec3, thickness: f32) -> Result<Self, SolidError> {
        let [a, b, c] = corners;
        let mut normal = (b - a).cross(c - a).try_normalize().ok_or(SolidError::DegenerateTriangle)?;
        if normal.dot(up) < 0.0 {
            normal = -normal;
        }
        let face = normal.dot(a);
        let mut planes = vec![Plane { normal, dist: face }, Plane { normal: -normal, dist: thickness - face }];
        for (p, q, opposite) in [(a, b, c), (b, c, a), (c, a, b)] {
            let mut outward = (q - p).cross(normal).try_normalize().ok_or(SolidError::DegenerateTriangle)?;
            if outward.dot(opposite - p) > 0.0 {
                outward = -outward;
            }
            planes.push(Plane { normal: outward, dist: outward.dot(p) });
        }
        let behind = normal * thickness;
        let bounds = [a, b, c, a - behind, b - behind, c - behind]
            .iter()
            .fold(Aabb::from_corners(a, a), |acc, v| acc.union(&Aabb::from_corners(*v, *v)));
        planes.extend(Self::axis_box(bounds).planes);
        Ok(Self { planes, bounds })
    }

    /// The hull placed in the world: each point mapped to `rotation * p + origin`,
    /// bounded by its triangles' planes and its axial bevels.
    pub fn from_hull(hull: &Hull, rotation: Mat3, origin: Vec3) -> Self {
        let placed: Vec<Vec3> = hull.points.iter().map(|p| rotation * *p + origin).collect();
        let centroid = placed.iter().copied().sum::<Vec3>() / placed.len() as f32;
        let mut planes: Vec<Plane> = Vec::with_capacity(hull.triangles.len() + 6);
        for [a, b, c] in &hull.triangles {
            let (a, b, c) = (placed[*a], placed[*b], placed[*c]);
            let Some(mut normal) = (b - a).cross(c - a).try_normalize() else {
                continue; // a sliver triangle bounds nothing
            };
            if normal.dot(centroid - a) > 0.0 {
                normal = -normal;
            }
            let plane = Plane { normal, dist: normal.dot(a) };
            let repeats = planes
                .iter()
                .any(|q| q.normal.dot(plane.normal) > AXIAL && (q.dist - plane.dist).abs() < CONTACT_EPSILON);
            if !repeats {
                planes.push(plane);
            }
        }
        let bounds = placed
            .iter()
            .fold(Aabb::from_corners(placed[0], placed[0]), |acc, p| acc.union(&Aabb::from_corners(*p, *p)));
        planes.extend(Self::axis_box(bounds).planes);
        Self { planes, bounds }
    }

    pub fn translated(&self, by: Vec3) -> Self {
        Self {
            planes: self.planes.iter().map(|p| p.translated(by)).collect(),
            bounds: Aabb::from_corners(self.bounds.min() + by, self.bounds.max() + by),
        }
    }

    pub fn bounds(&self) -> Aabb {
        self.bounds
    }

    pub fn planes(&self) -> &[Plane] {
        &self.planes
    }

    /// Where the vertical line through `(x, y)` passes through the solid,
    /// counting points on the boundary as inside.
    pub fn vertical_span(&self, x: f32, y: f32) -> Option<Span> {
        let (mut bottom, mut top) = (f32::NEG_INFINITY, f32::INFINITY);
        let mut top_normal = Vec3::Z;
        for plane in &self.planes {
            let n = plane.normal;
            let level = n.x * x + n.y * y - plane.dist;
            if n.z.abs() < 1e-6 {
                if level > CONTACT_EPSILON {
                    return None;
                }
                continue;
            }
            let boundary = -level / n.z;
            if n.z > 0.0 {
                if boundary < top {
                    top = boundary;
                    top_normal = n;
                }
            } else {
                bottom = bottom.max(boundary);
            }
        }
        (bottom <= top + CONTACT_EPSILON).then_some(Span { bottom, top, top_normal })
    }

    /// Whether an axial box of `half` extents, swept from `from` to `to`
    /// (box centers), overlaps the solid's interior anywhere on the way.
    /// Touching the surface does not count.
    pub fn blocks_box_sweep(&self, half: Vec3, from: Vec3, to: Vec3) -> bool {
        self.sweep_entry(half, from, to).is_some()
    }

    /// How far along the sweep, from 0 to 1, the box first enters the
    /// solid's interior; `None` when it never does. A box starting inside
    /// enters at 0.
    pub fn sweep_entry(&self, half: Vec3, from: Vec3, to: Vec3) -> Option<f32> {
        // Minkowski sum: push each plane out by the box's support distance,
        // then clip the center segment against the grown solid.
        let (mut enter, mut leave) = (0.0f32, 1.0f32);
        for plane in &self.planes {
            let reach = plane.normal.abs().dot(half.abs());
            let start = plane.signed_distance(from) - reach;
            let end = plane.signed_distance(to) - reach;
            let start_inside = start < -CONTACT_EPSILON;
            let end_inside = end < -CONTACT_EPSILON;
            match (start_inside, end_inside) {
                (false, false) => return None,
                (true, true) => continue,
                (false, true) => enter = enter.max(start / (start - end)),
                (true, false) => leave = leave.min(start / (start - end)),
            }
            if enter >= leave {
                return None;
            }
        }
        Some(enter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 100.0, 10.0)))
    }

    #[test]
    fn a_box_has_the_bounds_it_was_built_from() {
        let b = unit_box();
        assert_eq!(b.bounds().min(), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(b.bounds().max(), Vec3::new(100.0, 100.0, 10.0));
        assert_eq!(b.planes().len(), 6);
    }

    #[test]
    fn planes_without_an_axial_ceiling_are_unbounded() {
        let planes = vec![
            Plane { normal: Vec3::new(1.0, 0.0, 0.0), dist: 1.0 },
            Plane { normal: Vec3::new(-1.0, 0.0, 0.0), dist: 1.0 },
            Plane { normal: Vec3::new(0.0, 1.0, 0.0), dist: 1.0 },
            Plane { normal: Vec3::new(0.0, -1.0, 0.0), dist: 1.0 },
            Plane { normal: Vec3::new(0.0, 0.0, -1.0), dist: 1.0 },
        ];
        assert_eq!(ConvexSolid::new(planes), Err(SolidError::Unbounded(AxialSide::MaxZ)));
    }

    #[test]
    fn a_column_through_a_box_spans_its_height_with_an_upward_top() {
        let span = unit_box().vertical_span(50.0, 50.0).expect("column is inside");
        assert_eq!((span.bottom, span.top), (0.0, 10.0));
        assert_eq!(span.top_normal, Vec3::Z);
    }

    #[test]
    fn a_column_on_the_boundary_still_hits_the_box() {
        assert!(unit_box().vertical_span(100.0, 0.0).is_some());
    }

    #[test]
    fn a_column_outside_misses() {
        assert_eq!(unit_box().vertical_span(150.0, 50.0), None);
    }

    #[test]
    fn a_column_through_a_ramp_tops_out_on_the_slope() {
        // A 45 degree wedge rising toward +x: z <= x, inside the unit box.
        let slope = Vec3::new(-1.0, 0.0, 1.0).try_normalize().expect("nonzero");
        let mut planes = unit_box().planes().to_vec();
        planes.push(Plane { normal: slope, dist: 0.0 });
        let ramp = ConvexSolid::new(planes).expect("bounded");
        let span = ramp.vertical_span(4.0, 50.0).expect("inside");
        assert!((span.top - 4.0).abs() < 1e-4, "top was {}", span.top);
        assert_eq!(span.top_normal, slope);
    }

    #[test]
    fn a_box_resting_on_top_only_touches() {
        let half = Vec3::new(16.0, 16.0, 10.0);
        let resting = Vec3::new(50.0, 50.0, 20.0);
        assert!(!unit_box().blocks_box_sweep(half, resting, resting));
    }

    #[test]
    fn a_box_sunk_into_the_top_overlaps() {
        let half = Vec3::new(16.0, 16.0, 10.0);
        let sunk = Vec3::new(50.0, 50.0, 19.0);
        assert!(unit_box().blocks_box_sweep(half, sunk, sunk));
    }

    #[test]
    fn a_sweep_through_the_box_is_blocked_even_when_both_ends_are_clear() {
        let half = Vec3::new(1.0, 1.0, 1.0);
        let before = Vec3::new(-50.0, 50.0, 5.0);
        let after = Vec3::new(150.0, 50.0, 5.0);
        assert!(unit_box().blocks_box_sweep(half, before, after));
    }

    #[test]
    fn a_sweep_alongside_the_box_is_clear() {
        let half = Vec3::new(16.0, 16.0, 1.0);
        let start = Vec3::new(-16.0, -50.0, 5.0);
        let end = Vec3::new(-16.0, 150.0, 5.0);
        assert!(!unit_box().blocks_box_sweep(half, start, end));
    }

    #[test]
    fn translation_moves_the_bounds() {
        let moved = unit_box().translated(Vec3::new(10.0, 0.0, -10.0));
        assert_eq!(moved.bounds().min(), Vec3::new(10.0, 0.0, -10.0));
        assert!(moved.vertical_span(105.0, 50.0).is_some());
    }

    #[test]
    fn a_triangle_prism_tops_out_on_the_triangle_facing_up() {
        // Wound clockwise from above, so its geometric normal points down.
        let tri = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 100.0, 0.0), Vec3::new(100.0, 0.0, 0.0)];
        let prism = ConvexSolid::from_triangle(tri, Vec3::Z, 4.0).expect("has area");
        let span = prism.vertical_span(10.0, 10.0).expect("inside");
        assert!((span.top - 0.0).abs() < 1e-4);
        assert!((span.bottom + 4.0).abs() < 1e-4);
        assert!((span.top_normal.z - 1.0).abs() < 1e-6);
        assert_eq!(prism.vertical_span(90.0, 90.0), None);
    }

    fn cube_hull() -> Hull {
        Hull::axis_box(Aabb::from_corners(Vec3::new(-8.0, -8.0, 0.0), Vec3::new(8.0, 8.0, 16.0)))
    }

    #[test]
    fn a_box_hull_has_eight_corners_and_twelve_faces() {
        let hull = cube_hull();
        assert_eq!((hull.points().len(), hull.triangles().len()), (8, 12));
    }

    #[test]
    fn a_hull_naming_a_missing_point_is_refused() {
        let result = Hull::new(vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], vec![[0, 1, 9]]);
        assert_eq!(result, Err(SolidError::HullIndex { triangle: 0, point: 9, points: 4 }));
    }

    #[test]
    fn a_flat_hull_is_refused() {
        let flat = vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::new(1.0, 1.0, 0.0)];
        assert_eq!(Hull::new(flat, vec![[0, 1, 2], [1, 3, 2]]), Err(SolidError::DegenerateHull));
    }

    #[test]
    fn a_placed_hull_is_moved_and_solid_inside() {
        let solid = ConvexSolid::from_hull(&cube_hull(), Mat3::IDENTITY, Vec3::new(100.0, 0.0, 50.0));
        assert_eq!(solid.bounds().min(), Vec3::new(92.0, -8.0, 50.0));
        let span = solid.vertical_span(100.0, 0.0).expect("inside");
        assert!((span.top - 66.0).abs() < 1e-4 && (span.bottom - 50.0).abs() < 1e-4);
        assert!(solid.vertical_span(109.0, 0.0).is_none());
    }

    #[test]
    fn a_hull_turned_45_degrees_reaches_its_corners_along_the_axes() {
        let turned = crate::core::geom::angle_matrix([0.0, 45.0, 0.0]);
        let solid = ConvexSolid::from_hull(&cube_hull(), turned, Vec3::ZERO);
        let corner = 8.0 * std::f32::consts::SQRT_2;
        assert!((solid.bounds().max().x - corner).abs() < 1e-3);
        assert!(solid.vertical_span(corner - 0.5, 0.0).is_some(), "the turned corner points along +x");
        assert!(solid.vertical_span(7.0, 7.0).is_none(), "the old corner is now outside");
    }

    #[test]
    fn a_sweep_enters_where_it_first_touches_the_interior() {
        let wall =
            ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(100.0, -50.0, 0.0), Vec3::new(116.0, 50.0, 100.0)));
        let entry = wall.sweep_entry(Vec3::ZERO, Vec3::new(0.0, 0.0, 50.0), Vec3::new(200.0, 0.0, 50.0)).expect("hits");
        assert!((entry - 0.5).abs() < 1e-4, "entry was {entry}");
        let inside = wall.sweep_entry(Vec3::ZERO, Vec3::new(108.0, 0.0, 50.0), Vec3::new(300.0, 0.0, 50.0));
        assert_eq!(inside, Some(0.0));
        assert_eq!(wall.sweep_entry(Vec3::ZERO, Vec3::new(0.0, 80.0, 50.0), Vec3::new(200.0, 80.0, 50.0)), None);
    }

    #[test]
    fn a_collinear_triangle_is_refused() {
        let tri = [Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)];
        assert_eq!(ConvexSolid::from_triangle(tri, Vec3::Z, 4.0), Err(SolidError::DegenerateTriangle));
    }
}
