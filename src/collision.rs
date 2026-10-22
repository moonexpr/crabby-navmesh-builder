//! The world as the generator sees it: vertical columns of solid, and box
//! overlap and sweep tests. [`SolidGrid`] answers these over a set of convex
//! solids, bucketed into a 2D grid so each query touches only nearby solids.

use crate::geom::{Aabb, Vec3};
use crate::solid::{ConvexSolid, Span};

/// What the generator needs from the world. `Sync` so that queries can run
/// from many threads at once.
pub trait Collision: Sync {
    /// Bounds of everything solid.
    fn bounds(&self) -> Aabb;

    /// The disjoint solid intervals on the vertical line through `(x, y)`,
    /// sorted bottom to top. Solids that overlap or touch merge into one span,
    /// whose `top_normal` is that of the solid reaching highest.
    fn column(&self, x: f32, y: f32) -> Vec<Span>;

    /// Whether `bounds` overlaps no solid's interior.
    fn box_clear(&self, bounds: Aabb) -> bool {
        self.sweep_clear(bounds.half_extents(), bounds.center(), bounds.center())
    }

    /// Whether a box of `half` extents moves from `from` to `to` (centers)
    /// without entering any solid.
    fn sweep_clear(&self, half: Vec3, from: Vec3, to: Vec3) -> bool;
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CollisionError {
    #[error("the map has no solid geometry to walk on")]
    Empty,
}

/// Solids bucketed by the grid cells their xy bounds overlap.
pub struct SolidGrid {
    solids: Vec<ConvexSolid>,
    bounds: Aabb,
    origin: (f32, f32),
    width: usize,
    depth: usize,
    buckets: Vec<Vec<u32>>,
}

/// Spans closer than this merge, so stacked brushes read as one floor.
const SPAN_MERGE_GAP: f32 = 0.1;

impl SolidGrid {
    /// Side length of a bucket, in world units.
    pub const BUCKET_SIZE: f32 = 128.0;

    pub fn new(solids: Vec<ConvexSolid>) -> Result<Self, CollisionError> {
        let bounds = solids.iter().map(ConvexSolid::bounds).reduce(|a, b| a.union(&b)).ok_or(CollisionError::Empty)?;
        let origin = (bounds.min().x, bounds.min().y);
        let width = ((bounds.max().x - origin.0) / Self::BUCKET_SIZE) as usize + 1;
        let depth = ((bounds.max().y - origin.1) / Self::BUCKET_SIZE) as usize + 1;
        let mut grid =
            Self { solids: Vec::new(), bounds, origin, width, depth, buckets: vec![Vec::new(); width * depth] };
        for (index, solid) in solids.iter().enumerate() {
            let (xs, ys) = grid.bucket_ranges(solid.bounds()).expect("a solid lies inside the bounds of all solids");
            for by in ys {
                for bx in xs.clone() {
                    grid.buckets[by * width + bx].push(index as u32);
                }
            }
        }
        grid.solids = solids;
        Ok(grid)
    }

    pub fn solid_count(&self) -> usize {
        self.solids.len()
    }

    /// The bucket columns and rows an xy region touches, or `None` when it
    /// lies wholly outside the world.
    fn bucket_ranges(&self, region: Aabb) -> Option<(std::ops::Range<usize>, std::ops::Range<usize>)> {
        let (min, max) = (region.min(), region.max());
        let (wmin, wmax) = (self.bounds.min(), self.bounds.max());
        if max.x < wmin.x || max.y < wmin.y || min.x > wmax.x || min.y > wmax.y {
            return None;
        }
        let cell = |v: f32, o: f32, n: usize| (((v - o) / Self::BUCKET_SIZE).max(0.0) as usize).min(n - 1);
        let xs = cell(min.x, self.origin.0, self.width)..cell(max.x, self.origin.0, self.width) + 1;
        let ys = cell(min.y, self.origin.1, self.depth)..cell(max.y, self.origin.1, self.depth) + 1;
        Some((xs, ys))
    }

    /// Every solid in the buckets a region touches, each once, in index order.
    fn candidates(&self, region: Aabb) -> Vec<u32> {
        let Some((xs, ys)) = self.bucket_ranges(region) else {
            return Vec::new();
        };
        let mut found: Vec<u32> = ys
            .flat_map(|by| xs.clone().map(move |bx| by * self.width + bx))
            .flat_map(|b| self.buckets[b].iter().copied())
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }
}

impl Collision for SolidGrid {
    fn bounds(&self) -> Aabb {
        self.bounds
    }

    fn column(&self, x: f32, y: f32) -> Vec<Span> {
        let point = Vec3::new(x, y, 0.0);
        let mut spans: Vec<Span> = self
            .candidates(Aabb::from_corners(point, point))
            .into_iter()
            .filter_map(|i| self.solids[i as usize].vertical_span(x, y))
            .collect();
        spans.sort_by(|a, b| a.bottom.total_cmp(&b.bottom));
        let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.bottom <= last.top + SPAN_MERGE_GAP => {
                    if span.top > last.top {
                        last.top = span.top;
                        last.top_normal = span.top_normal;
                    }
                }
                _ => merged.push(span),
            }
        }
        merged
    }

    fn sweep_clear(&self, half: Vec3, from: Vec3, to: Vec3) -> bool {
        let swept = Aabb::from_center(from, half).union(&Aabb::from_center(to, half));
        self.candidates(swept).into_iter().all(|i| !self.solids[i as usize].blocks_box_sweep(half, from, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slab(min: (f32, f32, f32), max: (f32, f32, f32)) -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2)))
    }

    #[test]
    fn an_empty_world_is_refused() {
        assert!(matches!(SolidGrid::new(vec![]), Err(CollisionError::Empty)));
    }

    #[test]
    fn stacked_touching_solids_merge_into_one_span() {
        let world =
            SolidGrid::new(vec![slab((0.0, 0.0, 0.0), (64.0, 64.0, 8.0)), slab((0.0, 0.0, 8.0), (64.0, 64.0, 16.0))])
                .expect("not empty");
        let spans = world.column(32.0, 32.0);
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0].bottom, spans[0].top), (0.0, 16.0));
    }

    #[test]
    fn a_floor_and_ceiling_are_two_spans_sorted_upward() {
        let world = SolidGrid::new(vec![
            slab((0.0, 0.0, 100.0), (64.0, 64.0, 110.0)),
            slab((0.0, 0.0, 0.0), (64.0, 64.0, 8.0)),
        ])
        .expect("not empty");
        let tops: Vec<f32> = world.column(32.0, 32.0).iter().map(|s| s.top).collect();
        assert_eq!(tops, vec![8.0, 110.0]);
    }

    #[test]
    fn solids_far_apart_in_different_buckets_are_both_found() {
        let world = SolidGrid::new(vec![
            slab((0.0, 0.0, 0.0), (64.0, 64.0, 8.0)),
            slab((5000.0, 5000.0, 0.0), (5064.0, 5064.0, 8.0)),
        ])
        .expect("not empty");
        assert_eq!(world.column(5032.0, 5032.0).len(), 1);
        assert_eq!(world.column(2000.0, 2000.0).len(), 0);
        let half = Vec3::new(16.0, 16.0, 16.0);
        assert!(!world.sweep_clear(half, Vec3::new(4900.0, 5032.0, 4.0), Vec3::new(5100.0, 5032.0, 4.0)));
    }

    #[test]
    fn a_wall_blocks_a_sweep_and_its_gap_does_not() {
        let world = SolidGrid::new(vec![
            slab((100.0, -500.0, 0.0), (116.0, 0.0, 200.0)),
            slab((100.0, 100.0, 0.0), (116.0, 500.0, 200.0)),
        ])
        .expect("not empty");
        let half = Vec3::new(16.0, 16.0, 18.0);
        let blocked = world.sweep_clear(half, Vec3::new(0.0, -100.0, 50.0), Vec3::new(200.0, -100.0, 50.0));
        let through_gap = world.sweep_clear(half, Vec3::new(0.0, 50.0, 50.0), Vec3::new(200.0, 50.0, 50.0));
        assert!(!blocked);
        assert!(through_gap);
    }

    #[test]
    fn box_clear_is_a_zero_length_sweep() {
        let world = SolidGrid::new(vec![slab((0.0, 0.0, 0.0), (64.0, 64.0, 8.0))]).expect("not empty");
        let above = Aabb::from_corners(Vec3::new(0.0, 0.0, 8.0), Vec3::new(32.0, 32.0, 40.0));
        let inside = Aabb::from_corners(Vec3::new(0.0, 0.0, 4.0), Vec3::new(32.0, 32.0, 40.0));
        assert!(world.box_clear(above));
        assert!(!world.box_clear(inside));
    }
}
