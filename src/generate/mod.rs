//! Builds nav areas from a [`Collision`] world.
//!
//! 1. [`heightfield`]: every grid column is cast through the world in
//!    parallel, keeping each floor where a player hull fits.
//! 2. [`links`]: each floor sample finds the samples it can walk, drop, or
//!    jump to in the four neighboring columns, in parallel.
//! 3. [`reach`]: a flood fill from the spawn points keeps what players reach.
//! 4. [`merge`]: grid cells whose corners are all walk-linked merge greedily
//!    into the largest near-planar rectangles.
//! 5. [`connect`]: areas connect along their edges, in parallel.

pub mod connect;
pub mod heightfield;
pub mod links;
pub mod merge;
pub mod reach;

use crate::collision::Collision;
use crate::geom::Vec3;
use crate::nav::{AreaId, Attributes, CornerHeights, NavArea};

use heightfield::Heightfield;
use links::Links;
use merge::Cells;

/// Agent and sampling dimensions, defaulting to `nav.h`'s values so the mesh
/// matches what `nav_generate` would build.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Grid spacing (`GenerationStepSize`).
    pub cell_size: f32,
    /// Height a player walks up without jumping (`StepHeight`).
    pub step_height: f32,
    /// Highest ledge reachable by a crouch jump (`JumpCrouchHeight`).
    pub jump_height: f32,
    /// Longest fall the mesh links (`DeathDrop`).
    pub max_drop: f32,
    /// Half the player hull's width (`HalfHumanWidth`).
    pub half_width: f32,
    /// Standing hull height (`HumanHeight`).
    pub stand_height: f32,
    /// Crouching hull height (`HumanCrouchHeight`).
    pub crouch_height: f32,
    /// Smallest floor normal z still walkable (`nav_slope_limit`).
    pub min_floor_normal_z: f32,
    /// How far a cell corner may sit from its merged area's surface.
    pub merge_tolerance: f32,
    /// How many columns away a drop or jump may land.
    pub leap_reach: u32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            cell_size: 25.0,
            step_height: 18.0,
            jump_height: 58.0,
            max_drop: 200.0,
            half_width: 16.0,
            stand_height: 71.0,
            crouch_height: 55.0,
            min_floor_normal_z: 0.7,
            merge_tolerance: 6.0,
            leap_reach: 2,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub columns: usize,
    pub samples: usize,
    pub reachable_samples: usize,
    pub cells: usize,
    pub areas: usize,
    pub seeds_used: usize,
    /// Seeds with no floor sample beneath them, which the fill skipped.
    pub seeds_off_mesh: Vec<Vec3>,
}

#[derive(Debug)]
pub struct Generated {
    pub areas: Vec<NavArea>,
    pub stats: Stats,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GenerateError {
    #[error("no seed points: the map has no spawn entities, so pass --seed x,y,z")]
    NoSeeds,
    #[error("none of the {0} seed points stands on a walkable floor")]
    NoSeedOnMesh(usize),
}

/// Generates the walkable areas reachable from `seeds` (player positions,
/// such as spawn origins). Runs on the current rayon thread pool, and gives
/// the same result whatever its size.
pub fn generate(world: &dyn Collision, seeds: &[Vec3], params: &Params) -> Result<Generated, GenerateError> {
    if seeds.is_empty() {
        return Err(GenerateError::NoSeeds);
    }
    let field = Heightfield::build(world, params);
    let resolved = reach::resolve_seeds(&field, seeds, params);
    if resolved.samples.is_empty() {
        return Err(GenerateError::NoSeedOnMesh(seeds.len()));
    }
    let links = Links::build(world, &field, params);
    let reachable = reach::reachable(&field, &links, &resolved.samples);
    let cells = Cells::build(&field, &links, &reachable);
    let rects = merge::merge(&cells, &field, params);
    let connections = connect::connect(&rects, &cells, &links);

    let area_id = |index: usize| AreaId::new(index as u32 + 1).expect("index + 1 is nonzero");
    let areas: Vec<NavArea> = rects
        .iter()
        .zip(connections)
        .enumerate()
        .map(|(index, (rect, links_out))| {
            let corner = |k: usize| field.sample(rect.corners[k]).position(field.grid());
            let (nw, se) = (corner(merge::NORTH_WEST), corner(merge::SOUTH_EAST));
            let heights = CornerHeights {
                north_west: nw.z,
                north_east: corner(merge::NORTH_EAST).z,
                south_east: se.z,
                south_west: corner(merge::SOUTH_WEST).z,
            };
            let attributes = if rect.crouch { Attributes::CROUCH } else { Attributes::NONE };
            NavArea::new(
                area_id(index),
                attributes,
                (nw.x, nw.y),
                (se.x, se.y),
                heights,
                links_out.map(|targets| targets.into_iter().map(area_id).collect()),
            )
            .expect("grid cells always span one cell east and south")
        })
        .collect();

    let stats = Stats {
        columns: field.grid().len(),
        samples: field.samples().len(),
        reachable_samples: reachable.iter().filter(|r| **r).count(),
        cells: cells.len(),
        areas: areas.len(),
        seeds_used: resolved.samples.len(),
        seeds_off_mesh: resolved.off_mesh,
    };
    Ok(Generated { areas, stats })
}
