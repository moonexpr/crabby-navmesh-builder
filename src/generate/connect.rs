//! Connections between merged rectangles. Rectangle `a` connects to `b` in
//! a direction when a sample on `a`'s edge that way is also a corner on
//! `b`'s facing edge, or links to one. Rectangles are independent here, so
//! they are connected in parallel.

use std::collections::HashSet;

use rayon::prelude::*;

use crate::nav::Direction;

use super::heightfield::SampleId;
use super::links::Links;
use super::merge::{Cells, NORTH_EAST, NORTH_WEST, Rect, SOUTH_EAST, SOUTH_WEST};

/// The two corners of a cell on its side facing `direction`.
fn side(direction: Direction) -> [usize; 2] {
    match direction {
        Direction::North => [NORTH_WEST, NORTH_EAST],
        Direction::East => [NORTH_EAST, SOUTH_EAST],
        Direction::South => [SOUTH_WEST, SOUTH_EAST],
        Direction::West => [NORTH_WEST, SOUTH_WEST],
    }
}

/// For each rectangle, the indices of rectangles it connects to in each
/// direction, sorted and without repeats.
pub fn connect(rects: &[Rect], cells: &Cells, links: &Links) -> Vec<[Vec<usize>; 4]> {
    let mut rect_of_cell = vec![0usize; cells.len()];
    for (index, rect) in rects.iter().enumerate() {
        for cell in &rect.cells {
            rect_of_cell[cell.0 as usize] = index;
        }
    }
    rects
        .par_iter()
        .enumerate()
        .map(|(index, rect)| {
            Direction::ALL.map(|direction| {
                let facing = side(direction.opposite());
                // The edge's samples and everywhere they lead in this direction;
                // a cell beyond shares the edge when its whole facing side is here.
                let reached: HashSet<SampleId> = rect
                    .edge(direction)
                    .iter()
                    .flat_map(|cell| side(direction).map(|k| cells.cell(*cell).corners[k]))
                    .flat_map(|s| std::iter::once(s).chain(links.from(s, direction).iter().map(|l| l.to)))
                    .collect();
                let mut targets: Vec<usize> = reached
                    .iter()
                    .flat_map(|sample| cells.with_corner(*sample).iter())
                    .filter(|(cell, k)| {
                        facing.contains(k) && facing.iter().all(|f| reached.contains(&cells.cell(*cell).corners[*f]))
                    })
                    .map(|(cell, _)| rect_of_cell[cell.0 as usize])
                    .filter(|other| *other != index)
                    .collect();
                targets.sort_unstable();
                targets.dedup();
                targets
            })
        })
        .collect()
}
