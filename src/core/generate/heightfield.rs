//! Floor samples on a regular grid. Each column is independent, so they are
//! cast in parallel; the samples come back in column order regardless.

use std::ops::Range;

use rayon::prelude::*;

use crate::core::collision::Collision;
use crate::core::geom::{Aabb, Vec3};

use super::Params;

/// A grid column, at world xy `(ix * cell_size, iy * cell_size)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Column {
    pub ix: i32,
    pub iy: i32,
}

impl Column {
    pub fn stepped(self, dx: i32, dy: i32) -> Column {
        Column { ix: self.ix + dx, iy: self.iy + dy }
    }
}

/// The columns covering the world's xy bounds, row by row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    origin: Column,
    width: u32,
    depth: u32,
    cell_size: f32,
}

impl Grid {
    pub fn covering(bounds: Aabb, cell_size: f32) -> Grid {
        let (min, max) = (bounds.min(), bounds.max());
        let origin = Column { ix: (min.x / cell_size).floor() as i32, iy: (min.y / cell_size).floor() as i32 };
        let width = ((max.x / cell_size).ceil() as i32 - origin.ix + 1) as u32;
        let depth = ((max.y / cell_size).ceil() as i32 - origin.iy + 1) as u32;
        Grid { origin, width, depth, cell_size }
    }

    pub fn len(&self) -> usize {
        self.width as usize * self.depth as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    pub fn index(&self, column: Column) -> Option<usize> {
        let dx = u32::try_from(column.ix - self.origin.ix).ok().filter(|dx| *dx < self.width)?;
        let dy = u32::try_from(column.iy - self.origin.iy).ok().filter(|dy| *dy < self.depth)?;
        Some(dy as usize * self.width as usize + dx as usize)
    }

    pub fn column(&self, index: usize) -> Column {
        let width = self.width as usize;
        self.origin.stepped((index % width) as i32, (index / width) as i32)
    }

    pub fn position(&self, column: Column) -> (f32, f32) {
        (column.ix as f32 * self.cell_size, column.iy as f32 * self.cell_size)
    }

    /// The column nearest a world xy position.
    pub fn nearest(&self, x: f32, y: f32) -> Column {
        Column { ix: (x / self.cell_size).round() as i32, iy: (y / self.cell_size).round() as i32 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SampleId(pub u32);

impl SampleId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A floor point where a player hull fits, crouching at least.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub column: Column,
    pub z: f32,
    pub normal: Vec3,
    pub crouch: bool,
}

impl Sample {
    pub fn position(&self, grid: &Grid) -> Vec3 {
        let (x, y) = grid.position(self.column);
        Vec3::new(x, y, self.z)
    }
}

/// A floor found on one column, before it has an id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Floor {
    pub z: f32,
    pub normal: Vec3,
    pub crouch: bool,
}

/// The walkable floors on the vertical line through `(x, y)`, lowest first:
/// tops of solid spans that face up within the slope limit and leave room
/// for a crouching hull standing on them.
pub fn floors(world: &dyn Collision, x: f32, y: f32, params: &Params) -> Vec<Floor> {
    let spans = world.column(x, y);
    let hull = |z: f32, height: f32| {
        Aabb::from_corners(
            Vec3::new(x - params.half_width, y - params.half_width, z + params.step_height),
            Vec3::new(x + params.half_width, y + params.half_width, z + height),
        )
    };
    spans
        .iter()
        .enumerate()
        .filter_map(|(i, span)| {
            let z = span.top;
            let ceiling = spans.get(i + 1).map_or(f32::INFINITY, |above| above.bottom);
            let walkable = span.top_normal.z >= params.min_floor_normal_z
                && ceiling - z >= params.crouch_height
                && world.box_clear(hull(z, params.crouch_height));
            walkable.then(|| Floor {
                z,
                normal: span.top_normal,
                crouch: !world.box_clear(hull(z, params.stand_height)),
            })
        })
        .collect()
}

pub struct Heightfield {
    grid: Grid,
    /// Column index to its range in `samples`; `column_starts[len]` ends it.
    column_starts: Vec<u32>,
    samples: Vec<Sample>,
}

impl Heightfield {
    pub fn build(world: &dyn Collision, params: &Params) -> Heightfield {
        let grid = Grid::covering(world.bounds(), params.cell_size);
        let per_column: Vec<Vec<Floor>> = (0..grid.len())
            .into_par_iter()
            .map(|index| {
                let (x, y) = grid.position(grid.column(index));
                floors(world, x, y, params)
            })
            .collect();
        let mut column_starts = Vec::with_capacity(grid.len() + 1);
        let mut samples = Vec::with_capacity(per_column.iter().map(Vec::len).sum());
        for (index, column_floors) in per_column.into_iter().enumerate() {
            column_starts.push(samples.len() as u32);
            let column = grid.column(index);
            samples.extend(column_floors.into_iter().map(|f| Sample {
                column,
                z: f.z,
                normal: f.normal,
                crouch: f.crouch,
            }));
        }
        column_starts.push(samples.len() as u32);
        Heightfield { grid, column_starts, samples }
    }

    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    pub fn sample(&self, id: SampleId) -> &Sample {
        &self.samples[id.index()]
    }

    /// Ids of the samples on a column, lowest first; empty off the grid.
    pub fn ids_at(&self, column: Column) -> Range<u32> {
        match self.grid.index(column) {
            Some(i) => self.column_starts[i]..self.column_starts[i + 1],
            None => 0..0,
        }
    }

    /// The highest sample within a column of `p`'s nearest that is no higher
    /// than `reach_up` above it, the floor a player at `p` stands on. Ties
    /// go to the column nearest `p`.
    pub fn floor_under(&self, p: Vec3, reach_up: f32) -> Option<SampleId> {
        let center = self.grid.nearest(p.x, p.y);
        let mut columns: Vec<Column> = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| center.stepped(dx, dy))).collect();
        columns.sort_by(|a, b| {
            let distance = |c: &Column| {
                let (x, y) = self.grid.position(*c);
                (x - p.x).powi(2) + (y - p.y).powi(2)
            };
            distance(a).total_cmp(&distance(b))
        });
        let mut best: Option<SampleId> = None;
        for column in columns {
            for id in self.ids_at(column).map(SampleId) {
                let z = self.sample(id).z;
                if z <= p.z + reach_up && best.is_none_or(|b| z > self.sample(b).z) {
                    best = Some(id);
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::collision::SolidGrid;
    use crate::core::solid::ConvexSolid;

    fn solid(min: (f32, f32, f32), max: (f32, f32, f32)) -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2)))
    }

    #[test]
    fn the_grid_covers_the_bounds_on_cell_multiples() {
        let grid = Grid::covering(Aabb::from_corners(Vec3::new(-30.0, 10.0, 0.0), Vec3::new(60.0, 50.0, 0.0)), 25.0);
        assert_eq!(grid.index(Column { ix: -2, iy: 0 }), Some(0));
        assert_eq!(grid.index(Column { ix: 3, iy: 2 }), Some(grid.len() - 1));
        assert_eq!(grid.index(Column { ix: 4, iy: 0 }), None);
        assert_eq!(grid.column(grid.len() - 1), Column { ix: 3, iy: 2 });
    }

    #[test]
    fn a_floor_with_headroom_is_found_and_one_without_is_not() {
        let world = SolidGrid::new(vec![
            solid((0.0, 0.0, -16.0), (200.0, 200.0, 0.0)),
            solid((0.0, 0.0, 100.0), (200.0, 200.0, 116.0)),
            solid((0.0, 0.0, 150.0), (200.0, 200.0, 166.0)),
        ])
        .expect("solid");
        let found = floors(&world, 100.0, 100.0, &Params::default());
        let zs: Vec<f32> = found.iter().map(|f| f.z).collect();
        // The gap above z = 116 is only 34 units: too low even to crouch.
        // The top of the stack has open sky above it.
        assert_eq!(zs, vec![0.0, 166.0]);
        assert!(found.iter().all(|f| !f.crouch));
    }

    #[test]
    fn a_floor_under_a_low_ceiling_is_crouch_only() {
        let world = SolidGrid::new(vec![
            solid((0.0, 0.0, -16.0), (200.0, 200.0, 0.0)),
            solid((0.0, 0.0, 60.0), (200.0, 200.0, 76.0)),
        ])
        .expect("solid");
        let found = floors(&world, 100.0, 100.0, &Params::default());
        assert!(found[0].crouch);
    }

    #[test]
    fn the_foot_of_a_wall_has_no_room_for_the_hull() {
        let world = SolidGrid::new(vec![
            solid((0.0, 0.0, -16.0), (200.0, 200.0, 0.0)),
            solid((110.0, 0.0, 0.0), (200.0, 200.0, 128.0)),
        ])
        .expect("solid");
        assert!(floors(&world, 100.0, 50.0, &Params::default()).iter().all(|f| f.z != 0.0));
        assert!(floors(&world, 90.0, 50.0, &Params::default()).iter().any(|f| f.z == 0.0));
    }

    #[test]
    fn a_floor_steeper_than_the_slope_limit_is_not_walkable() {
        let steep = Vec3::new(-1.0, 0.0, 0.5).try_normalize().expect("nonzero");
        let mut planes = solid((0.0, 0.0, -100.0), (200.0, 200.0, 300.0)).planes().to_vec();
        planes.push(crate::core::geom::Plane { normal: steep, dist: 0.0 });
        let wedge = ConvexSolid::new(planes).expect("bounded");
        let world = SolidGrid::new(vec![wedge]).expect("solid");
        assert!(floors(&world, 50.0, 100.0, &Params::default()).is_empty());
    }
}
