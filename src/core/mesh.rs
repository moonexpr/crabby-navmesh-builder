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

    pub const fn from_bits_const(bits: u32) -> Self {
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

/// `HidingSpot` flag bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HidingFlags(u8);

impl HidingFlags {
    pub const IN_COVER: HidingFlags = HidingFlags(0x01);
    pub const GOOD_SNIPER_SPOT: HidingFlags = HidingFlags(0x02);
    pub const IDEAL_SNIPER_SPOT: HidingFlags = HidingFlags(0x04);
    pub const EXPOSED: HidingFlags = HidingFlags(0x08);

    pub fn from_bits(bits: u8) -> Self {
        HidingFlags(bits)
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn contains(self, other: HidingFlags) -> bool {
        self.0 & other.0 == other.0
    }
}

/// A place a bot can hide, in some area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HidingSpot {
    /// Unique across the mesh.
    pub id: u32,
    pub position: Vec3,
    pub flags: HidingFlags,
}

/// How much of another area can be seen from an area, `CNavArea::VisibilityType`'s bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Visibility(u8);

impl Visibility {
    /// An explicit "not visible", which inheriting areas use to override their source.
    pub const NOT_VISIBLE: Visibility = Visibility(0x00);
    pub const POTENTIALLY: Visibility = Visibility(0x01);
    pub const COMPLETELY: Visibility = Visibility(0x02);

    pub fn from_bits(bits: u8) -> Self {
        Visibility(bits)
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn is_visible(self) -> bool {
        self.0 != 0
    }
}

/// One entry of an area's potentially visible set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleArea {
    pub area: AreaId,
    pub visibility: Visibility,
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
    hiding_spots: Vec<HidingSpot>,
    visible_areas: Vec<VisibleArea>,
    inherit_visibility_from: Option<AreaId>,
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
            hiding_spots: Vec::new(),
            visible_areas: Vec::new(),
            inherit_visibility_from: None,
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

    pub fn hiding_spots(&self) -> &[HidingSpot] {
        &self.hiding_spots
    }

    /// The areas visible from this one, beyond those it inherits.
    pub fn visible_areas(&self) -> &[VisibleArea] {
        &self.visible_areas
    }

    pub fn inherit_visibility_from(&self) -> Option<AreaId> {
        self.inherit_visibility_from
    }

    /// The same area with these hiding spots.
    pub fn with_hiding_spots(self, hiding_spots: Vec<HidingSpot>) -> Self {
        Self { hiding_spots, ..self }
    }

    /// The same area with this visible set, inheriting from no other area.
    pub fn with_visible_areas(self, visible_areas: Vec<VisibleArea>) -> Self {
        Self { visible_areas, inherit_visibility_from: None, ..self }
    }

    /// The same area with a visible set that overrides `from`'s.
    pub fn with_inherited_visibility(self, from: AreaId, overrides: Vec<VisibleArea>) -> Self {
        Self { visible_areas: overrides, inherit_visibility_from: Some(from), ..self }
    }

    /// The center of the area, the midpoint of its north-west and south-east corners as `CNavArea` takes it.
    pub fn center(&self) -> Vec3 {
        (self.north_west + self.south_east) * 0.5
    }

    pub fn size_x(&self) -> f32 {
        self.south_east.x - self.north_west.x
    }

    pub fn size_y(&self) -> f32 {
        self.south_east.y - self.north_west.y
    }

    /// The floor height at `(x, y)`, interpolated between the corners as
    /// `CNavArea::GetZ` does, with the point clamped onto the area.
    pub fn z_at(&self, x: f32, y: f32) -> f32 {
        let u = ((x - self.north_west.x) / self.size_x()).clamp(0.0, 1.0);
        let v = ((y - self.north_west.y) / self.size_y()).clamp(0.0, 1.0);
        let north = self.north_west.z + u * (self.north_east_z - self.north_west.z);
        let south = self.south_west_z + u * (self.south_east.z - self.south_west_z);
        north + v * (south - north)
    }

    /// A corner with its own height: `[north_west, north_east, south_east, south_west]`.
    pub fn corners(&self) -> [Vec3; 4] {
        let (nw, se) = (self.north_west, self.south_east);
        [nw, Vec3::new(se.x, nw.y, self.north_east_z), se, Vec3::new(nw.x, se.y, self.south_west_z)]
    }

    /// Whether `(x, y)` lies on the area, edges included.
    pub fn contains_xy(&self, x: f32, y: f32) -> bool {
        self.north_west.x <= x && x <= self.south_east.x && self.north_west.y <= y && y <= self.south_east.y
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
    fn heights_interpolate_across_the_area_and_clamp_off_it() {
        let heights = CornerHeights { north_west: 0.0, north_east: 10.0, south_east: 30.0, south_west: 20.0 };
        let a = NavArea::new(id(1), Attributes::NONE, (0.0, 0.0), (100.0, 100.0), heights, Default::default())
            .expect("valid");
        assert_eq!(a.z_at(0.0, 0.0), 0.0);
        assert_eq!(a.z_at(100.0, 0.0), 10.0);
        assert_eq!(a.z_at(50.0, 50.0), 15.0);
        assert_eq!(a.z_at(-50.0, 500.0), 20.0, "clamped to the south-west corner");
        assert_eq!(a.corners()[1], Vec3::new(100.0, 0.0, 10.0));
        assert_eq!(a.center(), Vec3::new(50.0, 50.0, 15.0));
    }

    #[test]
    fn opposite_directions_pair_up() {
        assert_eq!(Direction::North.opposite(), Direction::South);
        assert_eq!(Direction::West.opposite(), Direction::East);
    }
}
