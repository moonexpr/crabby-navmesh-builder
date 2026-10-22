//! The parse seam for `.bsp` files (versions 19 to 21). [`parse`] is the only
//! code that reads map bytes; it returns the map's contents as domain values
//! (player-blocking brushes by model, displacement prisms, typed entities),
//! so nothing downstream re-checks the file.

mod assemble;
mod displacement;
mod entities;
mod lumps;
mod props;
mod records;

pub use displacement::{DisplacementError, DisplacementSurface};
pub use entities::EntityError;
pub use lumps::LumpId;
pub use props::{normalize_path, pakfile_files};

use crate::core::solid::SolidError;
use crate::core::world::{MapContents, WorldError};

/// A compiled map reduced to what navigation needs.
#[derive(Debug)]
pub struct Map {
    pub version: u32,
    pub contents: MapContents,
    pub stats: MapStats,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapStats {
    pub static_props: usize,
    /// Player-blocking brushes in the world model.
    pub brushes: usize,
    /// Models besides the world, each a brush entity's.
    pub brush_models: usize,
    pub displacements: usize,
    pub displacement_triangles: usize,
}

/// Brush contents bits from `bspflags.h`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contents(u32);

impl Contents {
    pub const SOLID: Contents = Contents(0x1);
    pub const WINDOW: Contents = Contents(0x2);
    pub const GRATE: Contents = Contents(0x8);
    pub const MOVEABLE: Contents = Contents(0x4000);
    pub const PLAYER_CLIP: Contents = Contents(0x1_0000);
    /// `MASK_PLAYERSOLID_BRUSHONLY`, the mask `nav_generate` traces with.
    pub const PLAYER_SOLID: Contents =
        Contents(Self::SOLID.0 | Self::WINDOW.0 | Self::GRATE.0 | Self::MOVEABLE.0 | Self::PLAYER_CLIP.0);

    pub fn from_bits(bits: u32) -> Self {
        Contents(bits)
    }

    pub fn intersects(self, mask: Contents) -> bool {
        self.0 & mask.0 != 0
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BspError {
    #[error("file is {0} bytes, too short for a BSP header")]
    TooShort(usize),
    #[error("not a BSP file (ident {0:#010x}, expected \"VBSP\")")]
    BadIdent(u32),
    #[error("BSP version {0} is not supported (19 to 21)")]
    UnsupportedVersion(u32),
    #[error("{lump:?} lump points outside the file")]
    LumpOutOfBounds { lump: LumpId },
    #[error("{lump:?} lump is {len} bytes, not a multiple of its {record}-byte records")]
    LumpSize { lump: LumpId, len: usize, record: usize },
    #[error("{lump:?} lump is compressed but could not be decompressed: {reason}")]
    Lzma { lump: LumpId, reason: String },
    #[error("{what} {index} refers past the end of its lump")]
    BadIndex { what: &'static str, index: i64 },
    #[error("brush {brush}: {source}")]
    Brush { brush: usize, source: SolidError },
    #[error("displacement {index}: {source}")]
    Displacement { index: usize, source: DisplacementError },
    #[error(transparent)]
    Entities(#[from] EntityError),
    #[error(transparent)]
    World(#[from] WorldError),
    #[error("static props: {0}")]
    StaticProps(String),
    #[error("pakfile: {0}")]
    Pakfile(String),
}

pub fn parse(bytes: &[u8]) -> Result<Map, BspError> {
    let lumps = lumps::Lumps::read(bytes)?;
    let props = props::static_props(bytes)?;
    assemble::assemble(&lumps, props)
}
