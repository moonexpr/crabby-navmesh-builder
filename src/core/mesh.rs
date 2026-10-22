//! The nav mesh as a value, whatever file it came from or goes to: areas,
//! their corners, attributes, and connections.

use std::num::NonZeroU32;

use crate::core::geom::Vec3;

/// An area's id. The engine reserves 0 for "no area".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AreaId(NonZeroU32);

impl AreaId {
    pub fn new(id: u32) -> Option<Self> {
        NonZeroU32::new(id).map(AreaId)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// Compass directions in the engine's order. North is -y.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}

impl Direction {
    pub const ALL: [Direction; 4] = [Direction::North, Direction::East, Direction::South, Direction::West];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn opposite(self) -> Direction {
        Direction::ALL[(self.index() + 2) % 4]
    }

    /// Grid step in this direction as `(dx, dy)`.
    pub fn offset(self) -> (i32, i32) {
        match self {
            Direction::North => (0, -1),
            Direction::East => (1, 0),
            Direction::South => (0, 1),
            Direction::West => (-1, 0),
        }
    }
}

/// `NavAttributeType` bit flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Attributes(u32);

impl Attributes {
    pub const NONE: Attributes = Attributes(0);
    pub const CROUCH: Attributes = Attributes(0x0000_0001);
    pub const JUMP: Attributes = Attributes(0x0000_0002);

    pub fn from_bits(bits: u32) -> Self {
        Attributes(bits)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn contains(self, other: Attributes) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn with(self, other: Attributes) -> Attributes {
        Attributes(self.0 | other.0)
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AreaError {
    #[error("area {id}: north-west corner ({nx}, {ny}) is not north-west of south-east corner ({sx}, {sy})")]
    InvertedExtent { id: u32, nx: f32, ny: f32, sx: f32, sy: f32 },
}

/// The four corner heights of an area, named by compass corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerHeights {
    pub north_west: f32,
    pub north_east: f32,
    pub south_east: f32,
    pub south_west: f32,
}

/// A rectangle in xy whose four corners may sit at different heights.
#[derive(Clone, Debug, PartialEq)]
pub struct NavArea {
    id: AreaId,
    attributes: Attributes,
    north_west: Vec3,
    south_east: Vec3,
    north_east_z: f32,
    south_west_z: f32,
    connections: [Vec<AreaId>; 4],
}

impl NavArea {
    /// `min_xy` is the north-west corner and `max_xy` the south-east; the
    /// rectangle must have positive width and depth.
    pub fn new(
        id: AreaId,
        attributes: Attributes,
        min_xy: (f32, f32),
        max_xy: (f32, f32),
        heights: CornerHeights,
        connections: [Vec<AreaId>; 4],
    ) -> Result<Self, AreaError> {
        if !(min_xy.0 < max_xy.0 && min_xy.1 < max_xy.1) {
            return Err(AreaError::InvertedExtent {
                id: id.get(),
                nx: min_xy.0,
                ny: min_xy.1,
                sx: max_xy.0,
                sy: max_xy.1,
            });
        }
        Ok(Self {
            id,
            attributes,
            north_west: Vec3::new(min_xy.0, min_xy.1, heights.north_west),
            south_east: Vec3::new(max_xy.0, max_xy.1, heights.south_east),
            north_east_z: heights.north_east,
            south_west_z: heights.south_west,
            connections,
        })
    }

    pub fn id(&self) -> AreaId {
        self.id
    }

    pub fn attributes(&self) -> Attributes {
        self.attributes
    }

    pub fn north_west(&self) -> Vec3 {
        self.north_west
    }

    pub fn south_east(&self) -> Vec3 {
        self.south_east
    }

    pub fn heights(&self) -> CornerHeights {
        CornerHeights {
            north_west: self.north_west.z,
            north_east: self.north_east_z,
            south_east: self.south_east.z,
            south_west: self.south_west_z,
        }
    }

    pub fn connections(&self, direction: Direction) -> &[AreaId] {
        &self.connections[direction.index()]
    }
}

/// A whole mesh: its areas and what the engine checks it against.
#[derive(Clone, Debug, PartialEq)]
pub struct NavMesh {
    /// Size in bytes of the `.bsp` the mesh was built from; the engine warns
    /// when it no longer matches.
    pub bsp_size: u32,
    pub analyzed: bool,
    pub areas: Vec<NavArea>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> AreaId {
        AreaId::new(n).expect("nonzero")
    }

    fn flat(z: f32) -> CornerHeights {
        CornerHeights { north_west: z, north_east: z, south_east: z, south_west: z }
    }

    #[test]
    fn area_zero_is_reserved() {
        assert_eq!(AreaId::new(0), None);
    }

    #[test]
    fn an_area_must_have_positive_extent() {
        let result = NavArea::new(id(1), Attributes::NONE, (10.0, 0.0), (0.0, 25.0), flat(0.0), Default::default());
        assert!(matches!(result, Err(AreaError::InvertedExtent { id: 1, .. })));
    }

    #[test]
    fn corners_keep_their_own_heights() {
        let heights = CornerHeights { north_west: 1.0, north_east: 2.0, south_east: 3.0, south_west: 4.0 };
        let a = NavArea::new(id(1), Attributes::NONE, (0.0, 0.0), (25.0, 25.0), heights, Default::default())
            .expect("valid");
        assert_eq!(a.heights(), heights);
        assert_eq!(a.north_west(), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(a.south_east(), Vec3::new(25.0, 25.0, 3.0));
    }

    #[test]
    fn opposite_directions_pair_up() {
        assert_eq!(Direction::North.opposite(), Direction::South);
        assert_eq!(Direction::West.opposite(), Direction::East);
    }
}
