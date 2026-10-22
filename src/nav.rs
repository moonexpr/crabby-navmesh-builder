//! The `.nav` file format, version 16, as written by `CNavMesh::Save` and
//! extended by `CTFNavArea::Save` (sub-version 2: a trailing u32 of TF
//! attributes per area). All values are little-endian.
//!
//! Generated meshes carry no places, hiding spots, encounter paths, ladders,
//! or visibility; `nav_analyze` in game fills those in. The decoder reads all
//! of them structurally, so Valve's own files can be inspected, and reports
//! how much of each it skipped.

use std::num::NonZeroU32;

use crate::geom::Vec3;

pub const MAGIC: u32 = 0xFEED_FACE;
pub const VERSION: u32 = 16;
pub const TF_SUB_VERSION: u32 = 2;

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

/// Everything a generated `.nav` file holds.
#[derive(Clone, Debug, PartialEq)]
pub struct NavFile {
    /// Size in bytes of the `.bsp` the mesh was built from; the engine warns
    /// when it no longer matches.
    pub bsp_size: u32,
    pub analyzed: bool,
    pub areas: Vec<NavArea>,
}

/// Counts of data the decoder read past without keeping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub places: usize,
    pub hiding_spots: usize,
    pub encounter_paths: usize,
    pub ladders: usize,
    pub visible_area_entries: usize,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NavError {
    #[error("not a nav file (magic {0:#010x})")]
    BadMagic(u32),
    #[error("nav version {0} is not supported (only {VERSION})")]
    UnsupportedVersion(u32),
    #[error("nav sub-version {0} is not supported (only TF's {TF_SUB_VERSION})")]
    UnsupportedSubVersion(u32),
    #[error("file ends early at byte {0}")]
    Truncated(usize),
    #[error("{0} bytes left over after the last field")]
    TrailingBytes(usize),
    #[error("an area id is 0, which the engine reserves for no area")]
    ZeroAreaId,
    #[error(transparent)]
    Area(#[from] AreaError),
}

/// Light intensity written for every corner; the engine relights on analysis.
const FULL_LIGHT: f32 = 1.0;
const MAX_NAV_TEAMS: usize = 2;
const NUM_LADDER_DIRECTIONS: usize = 2;
const HIDING_SPOT_SIZE: usize = 4 + 12 + 1;
const LADDER_SIZE: usize = 4 + 4 + 12 + 12 + 4 + 4 + 5 * 4;

pub fn encode(file: &NavFile) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + file.areas.len() * 128);
    put_u32(&mut out, MAGIC);
    put_u32(&mut out, VERSION);
    put_u32(&mut out, TF_SUB_VERSION);
    put_u32(&mut out, file.bsp_size);
    out.push(u8::from(file.analyzed));
    out.extend_from_slice(&0u16.to_le_bytes()); // no named places
    out.push(1); // so every area is unnamed
    put_u32(&mut out, file.areas.len() as u32);
    for area in &file.areas {
        put_u32(&mut out, area.id.get());
        put_u32(&mut out, area.attributes.bits());
        for v in [area.north_west, area.south_east] {
            for c in [v.x, v.y, v.z] {
                put_f32(&mut out, c);
            }
        }
        put_f32(&mut out, area.north_east_z);
        put_f32(&mut out, area.south_west_z);
        for direction in &area.connections {
            put_u32(&mut out, direction.len() as u32);
            for to in direction {
                put_u32(&mut out, to.get());
            }
        }
        out.push(0); // hiding spots
        put_u32(&mut out, 0); // encounter paths
        out.extend_from_slice(&0u16.to_le_bytes()); // place
        for _ in 0..NUM_LADDER_DIRECTIONS {
            put_u32(&mut out, 0);
        }
        for _ in 0..MAX_NAV_TEAMS {
            put_f32(&mut out, 0.0); // earliest occupy time
        }
        for _ in 0..4 {
            put_f32(&mut out, FULL_LIGHT);
        }
        put_u32(&mut out, 0); // potentially visible areas
        put_u32(&mut out, 0); // inherit visibility from
        put_u32(&mut out, 0); // TF attributes
    }
    put_u32(&mut out, 0); // ladders
    out
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NavError> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len()).ok_or(NavError::Truncated(self.at))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, NavError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, NavError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("took 2 bytes")))
    }

    fn u32(&mut self) -> Result<u32, NavError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("took 4 bytes")))
    }

    fn f32(&mut self) -> Result<f32, NavError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().expect("took 4 bytes")))
    }

    fn vec3(&mut self) -> Result<Vec3, NavError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    fn area_id(&mut self) -> Result<AreaId, NavError> {
        AreaId::new(self.u32()?).ok_or(NavError::ZeroAreaId)
    }
}

pub fn decode(bytes: &[u8]) -> Result<(NavFile, Skipped), NavError> {
    let mut r = Reader { bytes, at: 0 };
    let magic = r.u32()?;
    if magic != MAGIC {
        return Err(NavError::BadMagic(magic));
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(NavError::UnsupportedVersion(version));
    }
    let sub_version = r.u32()?;
    if sub_version != TF_SUB_VERSION {
        return Err(NavError::UnsupportedSubVersion(sub_version));
    }
    let bsp_size = r.u32()?;
    let analyzed = r.u8()? != 0;
    let mut skipped = Skipped { places: r.u16()? as usize, ..Skipped::default() };
    for _ in 0..skipped.places {
        let len = r.u16()? as usize;
        r.take(len)?;
    }
    r.u8()?; // has unnamed areas

    let area_count = r.u32()? as usize;
    let mut areas = Vec::with_capacity(area_count.min(bytes.len() / 64));
    for _ in 0..area_count {
        areas.push(decode_area(&mut r, &mut skipped)?);
    }

    skipped.ladders = r.u32()? as usize;
    r.take(skipped.ladders.checked_mul(LADDER_SIZE).ok_or(NavError::Truncated(r.at))?)?;

    let left = bytes.len() - r.at;
    if left != 0 {
        return Err(NavError::TrailingBytes(left));
    }
    Ok((NavFile { bsp_size, analyzed, areas }, skipped))
}

fn decode_area(r: &mut Reader<'_>, skipped: &mut Skipped) -> Result<NavArea, NavError> {
    let id = r.area_id()?;
    let attributes = Attributes::from_bits(r.u32()?);
    let north_west = r.vec3()?;
    let south_east = r.vec3()?;
    let north_east_z = r.f32()?;
    let south_west_z = r.f32()?;
    let mut connections: [Vec<AreaId>; 4] = Default::default();
    for direction in connections.iter_mut() {
        let count = r.u32()? as usize;
        for _ in 0..count {
            direction.push(r.area_id()?);
        }
    }

    let hiding_spots = r.u8()? as usize;
    skipped.hiding_spots += hiding_spots;
    r.take(hiding_spots * HIDING_SPOT_SIZE)?;

    let encounters = r.u32()? as usize;
    skipped.encounter_paths += encounters;
    for _ in 0..encounters {
        r.take(4 + 1 + 4 + 1)?;
        let spots = r.u8()? as usize;
        r.take(spots * (4 + 1))?;
    }

    r.u16()?; // place
    for _ in 0..NUM_LADDER_DIRECTIONS {
        let count = r.u32()? as usize;
        r.take(count.checked_mul(4).ok_or(NavError::Truncated(r.at))?)?;
    }
    r.take(MAX_NAV_TEAMS * 4 + 4 * 4)?; // occupy times, corner light

    let visible = r.u32()? as usize;
    skipped.visible_area_entries += visible;
    r.take(visible.checked_mul(4 + 1).ok_or(NavError::Truncated(r.at))?)?;
    r.u32()?; // inherit visibility from
    r.u32()?; // TF attributes

    let heights = CornerHeights {
        north_west: north_west.z,
        north_east: north_east_z,
        south_east: south_east.z,
        south_west: south_west_z,
    };
    Ok(NavArea::new(id, attributes, (north_west.x, north_west.y), (south_east.x, south_east.y), heights, connections)?)
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

    fn area(n: u32, min: (f32, f32), max: (f32, f32), connections: [Vec<AreaId>; 4]) -> NavArea {
        NavArea::new(id(n), Attributes::NONE, min, max, flat(0.0), connections).expect("valid extent")
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

    #[test]
    fn an_empty_mesh_encodes_to_the_exact_header() {
        let file = NavFile { bsp_size: 1234, analyzed: false, areas: vec![] };
        let mut expected = Vec::new();
        expected.extend_from_slice(&MAGIC.to_le_bytes());
        expected.extend_from_slice(&16u32.to_le_bytes());
        expected.extend_from_slice(&2u32.to_le_bytes());
        expected.extend_from_slice(&1234u32.to_le_bytes());
        expected.push(0); // not analyzed
        expected.extend_from_slice(&0u16.to_le_bytes()); // no places
        expected.push(1); // has unnamed areas
        expected.extend_from_slice(&0u32.to_le_bytes()); // areas
        expected.extend_from_slice(&0u32.to_le_bytes()); // ladders
        assert_eq!(encode(&file), expected);
    }

    #[test]
    fn one_area_encodes_to_the_engine_layout() {
        let a = area(7, (0.0, 0.0), (50.0, 25.0), [vec![], vec![id(9)], vec![], vec![]]);
        let bytes = encode(&NavFile { bsp_size: 0, analyzed: false, areas: vec![a] });
        let header = 4 + 4 + 4 + 4 + 1 + 2 + 1 + 4;
        let area_bytes = 4 + 4 + 24 + 8 // id, flags, corners, implicit heights
            + (4 + 4 + 4 + 4 + 4) // connection counts plus one id
            + 1 + 4 + 2 // hiding spots, encounters, place
            + 8 + 8 + 16 // ladder counts, occupy times, light
            + 4 + 4 // visibility count, inherit
            + 4; // TF attributes
        assert_eq!(bytes.len(), header + area_bytes + 4);
        let at = header;
        assert_eq!(&bytes[at..at + 4], &7u32.to_le_bytes());
        assert_eq!(&bytes[at + 20..at + 24], &50.0f32.to_le_bytes()); // se.x
        let east_count = at + 40 + 4;
        assert_eq!(&bytes[east_count..east_count + 4], &1u32.to_le_bytes());
        assert_eq!(&bytes[east_count + 4..east_count + 8], &9u32.to_le_bytes());
        let light = at + area_bytes - 4 - 8 - 16;
        assert_eq!(&bytes[light..light + 4], &1.0f32.to_le_bytes());
    }

    #[test]
    fn encoding_round_trips_through_decode() {
        let crouch = NavArea::new(
            id(2),
            Attributes::CROUCH,
            (25.0, 0.0),
            (50.0, 25.0),
            CornerHeights { north_west: 0.0, north_east: 4.0, south_east: 8.0, south_west: 2.0 },
            [vec![], vec![], vec![], vec![id(1)]],
        )
        .expect("valid");
        let file = NavFile {
            bsp_size: 99,
            analyzed: true,
            areas: vec![area(1, (0.0, 0.0), (25.0, 25.0), [vec![], vec![id(2)], vec![], vec![]]), crouch],
        };
        let (decoded, skipped) = decode(&encode(&file)).expect("decodes");
        assert_eq!(decoded, file);
        assert_eq!(skipped, Skipped::default());
    }

    #[test]
    fn decode_refuses_other_files() {
        assert_eq!(decode(&[0, 1, 2, 3, 4, 5, 6, 7]), Err(NavError::BadMagic(0x0302_0100)));
        let mut old = MAGIC.to_le_bytes().to_vec();
        old.extend_from_slice(&15u32.to_le_bytes());
        assert_eq!(decode(&old), Err(NavError::UnsupportedVersion(15)));
    }

    #[test]
    fn decode_reports_truncation() {
        let bytes = encode(&NavFile { bsp_size: 0, analyzed: false, areas: vec![] });
        assert!(matches!(decode(&bytes[..bytes.len() - 2]), Err(NavError::Truncated(_))));
    }
}
