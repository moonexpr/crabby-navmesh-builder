//! Moves between floor samples in neighboring columns, each checked with a
//! hull sweep. Every sample's links are independent, so they are built in
//! parallel.

use rayon::prelude::*;

use crate::core::collision::Collision;
use crate::core::geom::Vec3;
use crate::core::mesh::Direction;

use super::Params;
use super::heightfield::{Heightfield, SampleId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// Within a step's height either way, to the adjacent column.
    Walk,
    /// Falling to a lower floor.
    Drop,
    /// Jumping up to a ledge.
    Jump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub to: SampleId,
    pub kind: LinkKind,
}

pub struct Links {
    by_sample: Vec<[Vec<Link>; 4]>,
}

impl Links {
    pub fn build(world: &dyn Collision, field: &Heightfield, params: &Params) -> Links {
        let by_sample = (0..field.samples().len() as u32)
            .into_par_iter()
            .map(|i| Direction::ALL.map(|d| links_toward(world, field, params, SampleId(i), d)))
            .collect();
        Links { by_sample }
    }

    pub fn from(&self, sample: SampleId, direction: Direction) -> &[Link] {
        &self.by_sample[sample.index()][direction.index()]
    }

    /// The sample one column over that `sample` walks to, if any.
    pub fn walk(&self, sample: SampleId, direction: Direction) -> Option<SampleId> {
        self.from(sample, direction).iter().find(|l| l.kind == LinkKind::Walk).map(|l| l.to)
    }
}

/// The links from `from` in one direction. The nearest column holding any
/// link wins; columns past the first only offer drops and jumps, since a
/// level gap a column wide is a jump across this crate doesn't model.
fn links_toward(
    world: &dyn Collision,
    field: &Heightfield,
    params: &Params,
    from: SampleId,
    direction: Direction,
) -> Vec<Link> {
    let start = field.sample(from);
    let (dx, dy) = direction.offset();
    for distance in 1..=params.leap_reach.max(1) as i32 {
        let column = start.column.stepped(dx * distance, dy * distance);
        let candidates: Vec<SampleId> = field.ids_at(column).map(SampleId).collect();
        if candidates.is_empty() {
            continue;
        }
        let z = |id: &SampleId| field.sample(*id).z;
        let mut found = Vec::new();
        // Walking or falling lands on the highest floor below a step up.
        if let Some(to) =
            candidates.iter().filter(|id| z(id) <= start.z + params.step_height).max_by(|a, b| z(a).total_cmp(&z(b)))
        {
            let rise = z(to) - start.z;
            let kind = if rise >= -params.step_height { LinkKind::Walk } else { LinkKind::Drop };
            let in_range = match kind {
                LinkKind::Walk => distance == 1,
                _ => rise >= -params.max_drop,
            };
            if in_range && path_clear(world, field, params, from, *to) {
                found.push(Link { to: *to, kind });
            }
        }
        // Jumping lands on the lowest ledge within reach.
        if let Some(to) = candidates
            .iter()
            .filter(|id| z(id) > start.z + params.step_height && z(id) <= start.z + params.jump_height)
            .min_by(|a, b| z(a).total_cmp(&z(b)))
            && path_clear(world, field, params, from, *to)
        {
            found.push(Link { to: *to, kind: LinkKind::Jump });
        }
        if !found.is_empty() {
            return found;
        }
    }
    Vec::new()
}

/// Whether the hull, lifted a step off the floor, can rise or fall at each
/// end and cross between them at the higher of the two heights.
fn path_clear(world: &dyn Collision, field: &Heightfield, params: &Params, from: SampleId, to: SampleId) -> bool {
    let grid = field.grid();
    let (a, b) = (field.sample(from).position(grid), field.sample(to).position(grid));
    let half = Vec3::new(params.half_width, params.half_width, (params.crouch_height - params.step_height) * 0.5);
    let lift = Vec3::new(0.0, 0.0, params.step_height + half.z);
    let cross_z = a.z.max(b.z);
    let (a_up, b_up) = (Vec3::new(a.x, a.y, cross_z), Vec3::new(b.x, b.y, cross_z));
    world.sweep_clear(half, a + lift, a_up + lift)
        && world.sweep_clear(half, a_up + lift, b_up + lift)
        && world.sweep_clear(half, b_up + lift, b + lift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::collision::SolidGrid;
    use crate::core::generate::heightfield::Column;
    use crate::core::geom::{Aabb, Vec3};
    use crate::core::solid::ConvexSolid;

    fn solid(min: (f32, f32, f32), max: (f32, f32, f32)) -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2)))
    }

    fn only(field: &Heightfield, ix: i32, iy: i32) -> SampleId {
        let ids = field.ids_at(Column { ix, iy });
        assert_eq!(ids.len(), 1, "expected one sample at ({ix}, {iy})");
        SampleId(ids.start)
    }

    #[test]
    fn neighbors_on_a_flat_floor_walk_both_ways() {
        let world = SolidGrid::new(vec![solid((0.0, 0.0, -16.0), (200.0, 200.0, 0.0))]).expect("solid");
        let params = Params::default();
        let field = Heightfield::build(&world, &params);
        let links = Links::build(&world, &field, &params);
        let (a, b) = (only(&field, 2, 2), only(&field, 3, 2));
        assert_eq!(links.walk(a, Direction::East), Some(b));
        assert_eq!(links.walk(b, Direction::West), Some(a));
        assert_eq!(links.walk(a, Direction::North).map(|s| field.sample(s).column), Some(Column { ix: 2, iy: 1 }));
    }

    #[test]
    fn a_ledge_is_a_drop_one_way() {
        let world = SolidGrid::new(vec![
            solid((0.0, 0.0, -16.0), (400.0, 200.0, 0.0)),
            solid((0.0, 0.0, 0.0), (110.0, 200.0, 100.0)),
        ])
        .expect("solid");
        let params = Params::default();
        let field = Heightfield::build(&world, &params);
        let links = Links::build(&world, &field, &params);
        let top = only(&field, 4, 4);
        assert_eq!(field.sample(top).z, 100.0);
        let drops: Vec<_> = links.from(top, Direction::East).iter().filter(|l| l.kind == LinkKind::Drop).collect();
        assert!(!drops.is_empty(), "the ledge drops east");
        for drop in drops {
            assert_eq!(field.sample(drop.to).z, 0.0);
            let back = links.from(drop.to, Direction::West);
            assert!(back.iter().all(|l| l.to != top), "a 100 unit ledge is too high to jump back");
        }
    }

    #[test]
    fn a_wall_between_columns_blocks_the_walk() {
        let world = SolidGrid::new(vec![
            solid((0.0, 0.0, -16.0), (400.0, 200.0, 0.0)),
            solid((124.0, 0.0, 0.0), (126.0, 200.0, 200.0)),
        ])
        .expect("solid");
        let params = Params { half_width: 8.0, ..Params::default() };
        let field = Heightfield::build(&world, &params);
        let links = Links::build(&world, &field, &params);
        let west = only(&field, 4, 4);
        assert!(links.from(west, Direction::East).is_empty());
    }
}
