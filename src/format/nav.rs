//! The `.nav` file format, version 16: a parser from bytes to a
//! [`NavMesh`], and a compiler back. `CNavMesh::Save` fixes the base layout;
//! each game's dialect may add data after every area (TF2's
//! `CTFNavArea::Save` adds a u32 of TF attributes) and says so with its
//! sub-version. All values are little-endian.
//!
//! Compiled meshes carry no places, hiding spots, encounter paths, ladders,
//! or visibility; `nav_analyze` in game fills those in. The parser reads all
//! of them structurally, so Valve's own files can be inspected, and reports
//! how much of each it skipped.

use std::fmt::Debug;

use crate::core::geom::Vec3;
use crate::core::mesh::{AreaError, AreaId, Attributes, CornerHeights, NavArea, NavMesh};

pub const MAGIC: u32 = 0xFEED_FACE;
pub const VERSION: u32 = 16;

/// What a game adds to the base layout: the hooks the compiler and parser
/// call where `CNavArea::Save`'s fields end.
pub trait NavDialect: Debug + Send + Sync {
    /// The sub-version this dialect writes, and is recognized by.
    fn sub_version(&self) -> u32;

    /// Bytes this dialect appends after each area's base fields.
    fn area_tail_len(&self) -> usize;

    /// Appends exactly `area_tail_len` bytes for `area`.
    fn write_area_tail(&self, area: &NavArea, out: &mut Vec<u8>);
}

/// Counts of data the parser read past without keeping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Skipped {
    pub places: usize,
    pub hiding_spots: usize,
    pub encounter_paths: usize,
    pub ladders: usize,
    pub visible_area_entries: usize,
}

/// A parsed file: the mesh, the sub-version it was written at, and what was skipped.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub mesh: NavMesh,
    pub sub_version: u32,
    pub skipped: Skipped,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NavError {
    #[error("not a nav file (magic {0:#010x})")]
    BadMagic(u32),
    #[error("nav version {0} is not supported (only {VERSION})")]
    UnsupportedVersion(u32),
    #[error("nav sub-version {0} belongs to no supported build")]
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

/// The mesh as `dialect` writes it.
pub fn compile(mesh: &NavMesh, dialect: &dyn NavDialect) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + mesh.areas.len() * (124 + dialect.area_tail_len()));
    put_u32(&mut out, MAGIC);
    put_u32(&mut out, VERSION);
    put_u32(&mut out, dialect.sub_version());
    put_u32(&mut out, mesh.bsp_size);
    out.push(u8::from(mesh.analyzed));
    out.extend_from_slice(&0u16.to_le_bytes()); // no named places
    out.push(1); // so every area is unnamed
    put_u32(&mut out, mesh.areas.len() as u32);
    for area in &mesh.areas {
        compile_base_area(area, &mut out);
        let before = out.len();
        dialect.write_area_tail(area, &mut out);
        debug_assert_eq!(out.len() - before, dialect.area_tail_len(), "{dialect:?} wrote the wrong tail length");
    }
    put_u32(&mut out, 0); // ladders
    out
}

fn compile_base_area(area: &NavArea, out: &mut Vec<u8>) {
    let (nw, se, heights) = (area.north_west(), area.south_east(), area.heights());
    put_u32(out, area.id().get());
    put_u32(out, area.attributes().bits());
    for c in [nw.x, nw.y, nw.z, se.x, se.y, se.z, heights.north_east, heights.south_west] {
        put_f32(out, c);
    }
    for direction in crate::core::mesh::Direction::ALL {
        let targets = area.connections(direction);
        put_u32(out, targets.len() as u32);
        for to in targets {
            put_u32(out, to.get());
        }
    }
    out.push(0); // hiding spots
    put_u32(out, 0); // encounter paths
    out.extend_from_slice(&0u16.to_le_bytes()); // place
    for _ in 0..NUM_LADDER_DIRECTIONS {
        put_u32(out, 0);
    }
    for _ in 0..MAX_NAV_TEAMS {
        put_f32(out, 0.0); // earliest occupy time
    }
    for _ in 0..4 {
        put_f32(out, FULL_LIGHT);
    }
    put_u32(out, 0); // potentially visible areas
    put_u32(out, 0); // inherit visibility from
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

    fn counted(&mut self, count: usize, each: usize) -> Result<(), NavError> {
        let n = count.checked_mul(each).ok_or(NavError::Truncated(self.at))?;
        self.take(n).map(|_| ())
    }
}

/// Parses a version 16 file whose sub-version `dialect_for` recognizes.
pub fn parse<'d>(bytes: &[u8], dialect_for: impl Fn(u32) -> Option<&'d dyn NavDialect>) -> Result<Parsed, NavError> {
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
    let dialect = dialect_for(sub_version).ok_or(NavError::UnsupportedSubVersion(sub_version))?;
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
        areas.push(parse_area(&mut r, &mut skipped)?);
        r.take(dialect.area_tail_len())?;
    }

    skipped.ladders = r.u32()? as usize;
    r.counted(skipped.ladders, LADDER_SIZE)?;

    let left = bytes.len() - r.at;
    if left != 0 {
        return Err(NavError::TrailingBytes(left));
    }
    Ok(Parsed { mesh: NavMesh { bsp_size, analyzed, areas }, sub_version, skipped })
}

fn parse_area(r: &mut Reader<'_>, skipped: &mut Skipped) -> Result<NavArea, NavError> {
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
    r.counted(hiding_spots, HIDING_SPOT_SIZE)?;

    let encounters = r.u32()? as usize;
    skipped.encounter_paths += encounters;
    for _ in 0..encounters {
        r.take(4 + 1 + 4 + 1)?;
        let spots = r.u8()? as usize;
        r.counted(spots, 4 + 1)?;
    }

    r.u16()?; // place
    for _ in 0..NUM_LADDER_DIRECTIONS {
        let count = r.u32()? as usize;
        r.counted(count, 4)?;
    }
    r.take(MAX_NAV_TEAMS * 4 + 4 * 4)?; // occupy times, corner light

    let visible = r.u32()? as usize;
    skipped.visible_area_entries += visible;
    r.counted(visible, 4 + 1)?;
    r.u32()?; // inherit visibility from

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

    /// The base layout and nothing more.
    #[derive(Debug)]
    struct Base;

    impl NavDialect for Base {
        fn sub_version(&self) -> u32 {
            0
        }
        fn area_tail_len(&self) -> usize {
            0
        }
        fn write_area_tail(&self, _: &NavArea, _: &mut Vec<u8>) {}
    }

    /// A dialect that appends one u32 per area, as TF2's does.
    #[derive(Debug)]
    struct WithTail;

    impl NavDialect for WithTail {
        fn sub_version(&self) -> u32 {
            2
        }
        fn area_tail_len(&self) -> usize {
            4
        }
        fn write_area_tail(&self, area: &NavArea, out: &mut Vec<u8>) {
            put_u32(out, area.id().get() * 100);
        }
    }

    fn either(sub_version: u32) -> Option<&'static dyn NavDialect> {
        match sub_version {
            0 => Some(&Base),
            2 => Some(&WithTail),
            _ => None,
        }
    }

    fn id(n: u32) -> AreaId {
        AreaId::new(n).expect("nonzero")
    }

    fn area(n: u32, min: (f32, f32), max: (f32, f32), connections: [Vec<AreaId>; 4]) -> NavArea {
        let flat = CornerHeights { north_west: 0.0, north_east: 0.0, south_east: 0.0, south_west: 0.0 };
        NavArea::new(id(n), Attributes::NONE, min, max, flat, connections).expect("valid extent")
    }

    fn mesh(areas: Vec<NavArea>) -> NavMesh {
        NavMesh { bsp_size: 1234, analyzed: false, areas }
    }

    #[test]
    fn an_empty_mesh_compiles_to_the_exact_header_with_the_dialect_sub_version() {
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
        assert_eq!(compile(&mesh(vec![]), &WithTail), expected);
        assert_eq!(&compile(&mesh(vec![]), &Base)[8..12], &0u32.to_le_bytes());
    }

    #[test]
    fn one_area_compiles_to_the_engine_layout_then_the_dialect_tail() {
        let a = area(7, (0.0, 0.0), (50.0, 25.0), [vec![], vec![id(9)], vec![], vec![]]);
        let bytes = compile(&mesh(vec![a]), &WithTail);
        let header = 4 + 4 + 4 + 4 + 1 + 2 + 1 + 4;
        let base_area = 4 + 4 + 24 + 8 // id, flags, corners, implicit heights
            + (4 + 4 + 4 + 4 + 4) // connection counts plus one id
            + 1 + 4 + 2 // hiding spots, encounters, place
            + 8 + 8 + 16 // ladder counts, occupy times, light
            + 4 + 4; // visibility count, inherit
        assert_eq!(bytes.len(), header + base_area + 4 + 4);
        let at = header;
        assert_eq!(&bytes[at..at + 4], &7u32.to_le_bytes());
        assert_eq!(&bytes[at + 20..at + 24], &50.0f32.to_le_bytes()); // se.x
        assert_eq!(&bytes[at + 44..at + 48], &1u32.to_le_bytes()); // east count
        assert_eq!(&bytes[at + 48..at + 52], &9u32.to_le_bytes());
        let light = at + base_area - 8 - 16;
        assert_eq!(&bytes[light..light + 4], &1.0f32.to_le_bytes());
        let tail = at + base_area;
        assert_eq!(&bytes[tail..tail + 4], &700u32.to_le_bytes());
        assert_eq!(
            compile(&mesh(vec![area(7, (0.0, 0.0), (50.0, 25.0), Default::default())]), &Base).len(),
            header + base_area - 4 + 4
        );
    }

    #[test]
    fn each_dialect_round_trips_and_reports_its_sub_version() {
        let crouch = NavArea::new(
            id(2),
            Attributes::CROUCH,
            (25.0, 0.0),
            (50.0, 25.0),
            CornerHeights { north_west: 0.0, north_east: 4.0, south_east: 8.0, south_west: 2.0 },
            [vec![], vec![], vec![], vec![id(1)]],
        )
        .expect("valid");
        let original = NavMesh {
            bsp_size: 99,
            analyzed: true,
            areas: vec![area(1, (0.0, 0.0), (25.0, 25.0), [vec![], vec![id(2)], vec![], vec![]]), crouch],
        };
        for dialect in [&Base as &dyn NavDialect, &WithTail] {
            let parsed = parse(&compile(&original, dialect), either).expect("parses");
            assert_eq!(parsed.mesh, original);
            assert_eq!(parsed.sub_version, dialect.sub_version());
            assert_eq!(parsed.skipped, Skipped::default());
        }
    }

    #[test]
    fn parse_refuses_other_files_and_unknown_sub_versions() {
        assert_eq!(parse(&[0, 1, 2, 3, 4, 5, 6, 7], either), Err(NavError::BadMagic(0x0302_0100)));
        let mut old = MAGIC.to_le_bytes().to_vec();
        old.extend_from_slice(&15u32.to_le_bytes());
        assert_eq!(parse(&old, either), Err(NavError::UnsupportedVersion(15)));
        let mut css = compile(&mesh(vec![]), &Base);
        css[8..12].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(parse(&css, either), Err(NavError::UnsupportedSubVersion(1)));
    }

    #[test]
    fn parse_reports_truncation() {
        let bytes = compile(&mesh(vec![]), &Base);
        assert!(matches!(parse(&bytes[..bytes.len() - 2], either), Err(NavError::Truncated(_))));
    }
}
