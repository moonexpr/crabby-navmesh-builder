//! Which samples a player starting at a seed can reach.

use crate::geom::Vec3;
use crate::nav::Direction;

use super::Params;
use super::heightfield::{Heightfield, SampleId};
use super::links::Links;

/// Seeds resolved to the floors beneath them, and those with none.
pub struct Seeds {
    pub samples: Vec<SampleId>,
    pub off_mesh: Vec<Vec3>,
}

/// Spawn origins sit at or slightly above the floor, so a seed takes the
/// highest floor no more than a step above it.
pub fn resolve_seeds(field: &Heightfield, seeds: &[Vec3], params: &Params) -> Seeds {
    let mut resolved = Seeds { samples: Vec::new(), off_mesh: Vec::new() };
    for seed in seeds {
        match field.floor_under(*seed, params.step_height) {
            Some(sample) => resolved.samples.push(sample),
            None => resolved.off_mesh.push(*seed),
        }
    }
    resolved
}

/// A flood fill along links from `starts`, indexed by sample.
pub fn reachable(field: &Heightfield, links: &Links, starts: &[SampleId]) -> Vec<bool> {
    let mut seen = vec![false; field.samples().len()];
    let mut stack: Vec<SampleId> = Vec::with_capacity(starts.len());
    for start in starts {
        if !std::mem::replace(&mut seen[start.index()], true) {
            stack.push(*start);
        }
    }
    while let Some(sample) = stack.pop() {
        for direction in Direction::ALL {
            for link in links.from(sample, direction) {
                if !std::mem::replace(&mut seen[link.to.index()], true) {
                    stack.push(link.to);
                }
            }
        }
    }
    seen
}
