//! Grid cells, and their greedy merge into rectangles.
//!
//! A cell is the square between four reachable samples that walk to each
//! other around its edges. Cells merge into a rectangle while every corner
//! inside it stays within `merge_tolerance` of the surface its four outer
//! corners span, and while they agree on crouching.

use std::collections::HashMap;

use crate::core::mesh::Direction;

use super::Params;
use super::heightfield::{Heightfield, SampleId};
use super::links::Links;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CellId(pub u32);

/// Corner order in `Cell::corners` and `Rect::corners`.
pub const NORTH_WEST: usize = 0;
pub const NORTH_EAST: usize = 1;
pub const SOUTH_EAST: usize = 2;
pub const SOUTH_WEST: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub corners: [SampleId; 4],
    pub crouch: bool,
}

pub struct Cells {
    cells: Vec<Cell>,
    by_north_west: HashMap<SampleId, CellId>,
    /// For each sample, the cells it is a corner of, and which corner.
    by_corner: HashMap<SampleId, Vec<(CellId, usize)>>,
}

impl Cells {
    /// Every cell whose corners are all reachable, in sample order.
    pub fn build(field: &Heightfield, links: &Links, reachable: &[bool]) -> Cells {
        let mut cells = Cells { cells: Vec::new(), by_north_west: HashMap::new(), by_corner: HashMap::new() };
        for nw in (0..field.samples().len() as u32).map(SampleId) {
            let Some(corners) = cell_corners(links, nw) else {
                continue;
            };
            if !corners.iter().all(|c| reachable[c.index()]) {
                continue;
            }
            let id = CellId(cells.cells.len() as u32);
            let crouch = corners.iter().any(|c| field.sample(*c).crouch);
            cells.cells.push(Cell { corners, crouch });
            cells.by_north_west.insert(nw, id);
            for (k, corner) in corners.iter().enumerate() {
                cells.by_corner.entry(*corner).or_default().push((id, k));
            }
        }
        cells
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cell(&self, id: CellId) -> &Cell {
        &self.cells[id.0 as usize]
    }

    pub fn ids(&self) -> impl Iterator<Item = CellId> + '_ {
        (0..self.cells.len() as u32).map(CellId)
    }

    /// The cell sharing this cell's edge on the east or south side.
    pub fn neighbor(&self, id: CellId, direction: Direction) -> Option<CellId> {
        let corners = self.cell(id).corners;
        match direction {
            Direction::East => self.by_north_west.get(&corners[NORTH_EAST]).copied(),
            Direction::South => self.by_north_west.get(&corners[SOUTH_WEST]).copied(),
            Direction::West => self.find_with(corners[NORTH_WEST], NORTH_EAST, corners[SOUTH_WEST], SOUTH_EAST),
            Direction::North => self.find_with(corners[NORTH_WEST], SOUTH_WEST, corners[NORTH_EAST], SOUTH_EAST),
        }
    }

    /// The cell with `a` as corner `ka` and `b` as corner `kb`.
    fn find_with(&self, a: SampleId, ka: usize, b: SampleId, kb: usize) -> Option<CellId> {
        self.with_corner(a)
            .iter()
            .find(|(cell, k)| *k == ka && self.cell(*cell).corners[kb] == b)
            .map(|(cell, _)| *cell)
    }

    /// The cells that have `sample` as a corner, with the corner it is.
    pub fn with_corner(&self, sample: SampleId) -> &[(CellId, usize)] {
        self.by_corner.get(&sample).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// A merged rectangle of `width` by `depth` cells, row-major from north-west.
#[derive(Clone, Debug, PartialEq)]
pub struct Rect {
    pub cells: Vec<CellId>,
    pub width: u32,
    pub depth: u32,
    pub corners: [SampleId; 4],
    pub crouch: bool,
}

impl Rect {
    /// The cells along one edge, in order along it.
    pub fn edge(&self, direction: Direction) -> Vec<CellId> {
        let (w, d) = (self.width as usize, self.depth as usize);
        match direction {
            Direction::North => self.cells[..w].to_vec(),
            Direction::South => self.cells[(d - 1) * w..].to_vec(),
            Direction::West => (0..d).map(|r| self.cells[r * w]).collect(),
            Direction::East => (0..d).map(|r| self.cells[r * w + w - 1]).collect(),
        }
    }
}

/// The corners of the cell whose north-west corner is `nw`: each edge must
/// be walkable both ways, and both routes to the south-east must agree.
fn cell_corners(links: &Links, nw: SampleId) -> Option<[SampleId; 4]> {
    let ne = links.walk(nw, Direction::East)?;
    let sw = links.walk(nw, Direction::South)?;
    let se = links.walk(ne, Direction::South)?;
    let two_way = links.walk(sw, Direction::East) == Some(se)
        && links.walk(ne, Direction::West) == Some(nw)
        && links.walk(sw, Direction::North) == Some(nw)
        && links.walk(se, Direction::North) == Some(ne)
        && links.walk(se, Direction::West) == Some(sw);
    two_way.then_some([nw, ne, se, sw])
}

/// A rectangle being grown, as rows of cells.
struct Growing<'a> {
    rows: Vec<Vec<CellId>>,
    cells: &'a Cells,
    field: &'a Heightfield,
    tolerance: f32,
    crouch: bool,
}

impl Growing<'_> {
    fn open(&self, id: CellId, assigned: &[bool]) -> bool {
        !assigned[id.0 as usize] && self.cells.cell(id).crouch == self.crouch
    }

    /// The column east of the rectangle, if every cell of it may join.
    fn next_column(&self, assigned: &[bool]) -> Option<Vec<CellId>> {
        let mut column = Vec::with_capacity(self.rows.len());
        for (r, row) in self.rows.iter().enumerate() {
            let next = self.cells.neighbor(*row.last().expect("rows are never empty"), Direction::East)?;
            let stacks = r == 0 || self.cells.neighbor(column[r - 1], Direction::South) == Some(next);
            if !(self.open(next, assigned) && stacks) {
                return None;
            }
            column.push(next);
        }
        Some(column)
    }

    /// The row south of the rectangle, if every cell of it may join.
    fn next_row(&self, assigned: &[bool]) -> Option<Vec<CellId>> {
        let last = self.rows.last().expect("rows are never empty");
        let mut row = Vec::with_capacity(last.len());
        for (c, above) in last.iter().enumerate() {
            let next = self.cells.neighbor(*above, Direction::South)?;
            let joins = c == 0 || self.cells.neighbor(row[c - 1], Direction::East) == Some(next);
            if !(self.open(next, assigned) && joins) {
                return None;
            }
            row.push(next);
        }
        Some(row)
    }

    fn corners(&self) -> [SampleId; 4] {
        let first = &self.rows[0];
        let last = self.rows.last().expect("rows are never empty");
        let cell = |id: &CellId| self.cells.cell(*id).corners;
        [
            cell(&first[0])[NORTH_WEST],
            cell(first.last().expect("non-empty"))[NORTH_EAST],
            cell(last.last().expect("non-empty"))[SOUTH_EAST],
            cell(&last[0])[SOUTH_WEST],
        ]
    }

    /// Whether every cell corner lies within tolerance of the bilinear
    /// surface through the rectangle's outer corners.
    fn planar(&self) -> bool {
        let z = |s: SampleId| self.field.sample(s).z;
        let [nw, ne, se, sw] = self.corners().map(z);
        let (w, d) = (self.rows[0].len() as f32, self.rows.len() as f32);
        self.rows.iter().enumerate().all(|(r, row)| {
            row.iter().enumerate().all(|(c, id)| {
                let corners = self.cells.cell(*id).corners;
                [(0.0, 0.0, NORTH_WEST), (1.0, 0.0, NORTH_EAST), (1.0, 1.0, SOUTH_EAST), (0.0, 1.0, SOUTH_WEST)]
                    .iter()
                    .all(|(du, dv, k)| {
                        let (u, v) = ((c as f32 + du) / w, (r as f32 + dv) / d);
                        let north = nw + (ne - nw) * u;
                        let south = sw + (se - sw) * u;
                        (north + (south - north) * v - z(corners[*k])).abs() <= self.tolerance
                    })
            })
        })
    }

    /// Adds a column or row if the result stays planar; reports success.
    fn try_column(&mut self, assigned: &[bool]) -> bool {
        let Some(column) = self.next_column(assigned) else {
            return false;
        };
        for (row, cell) in self.rows.iter_mut().zip(&column) {
            row.push(*cell);
        }
        if self.planar() {
            return true;
        }
        for row in self.rows.iter_mut() {
            row.pop();
        }
        false
    }

    fn try_row(&mut self, assigned: &[bool]) -> bool {
        let Some(row) = self.next_row(assigned) else {
            return false;
        };
        self.rows.push(row);
        if self.planar() {
            return true;
        }
        self.rows.pop();
        false
    }
}

/// Covers every cell with exactly one rectangle, growing each from its
/// first unassigned cell a column and a row at a time so areas stay square.
pub fn merge(cells: &Cells, field: &Heightfield, params: &Params) -> Vec<Rect> {
    let mut assigned = vec![false; cells.len()];
    let mut rects = Vec::new();
    for start in cells.ids() {
        if assigned[start.0 as usize] {
            continue;
        }
        let mut growing = Growing {
            rows: vec![vec![start]],
            cells,
            field,
            tolerance: params.merge_tolerance,
            crouch: cells.cell(start).crouch,
        };
        loop {
            let grew_east = growing.try_column(&assigned);
            let grew_south = growing.try_row(&assigned);
            if !(grew_east || grew_south) {
                break;
            }
        }
        let corners = growing.corners();
        let (width, depth) = (growing.rows[0].len() as u32, growing.rows.len() as u32);
        let members: Vec<CellId> = growing.rows.into_iter().flatten().collect();
        for id in &members {
            assigned[id.0 as usize] = true;
        }
        rects.push(Rect { cells: members, width, depth, corners, crouch: cells.cell(start).crouch });
    }
    rects
}
