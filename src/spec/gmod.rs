//! Garry's Mod: the base `CNavMesh` layout at sub-version 0, which GMod's
//! `CNavArea::Load` reads with nothing after each area.

use crate::core::mesh::NavArea;
use crate::core::world::WorldPolicy;
use crate::format::nav::NavDialect;

use super::tf::TF2;
use super::{Build, GameFolder, GameSpec, SourcePolicy};

/// Garry's Mod's Steam app id.
pub const GARRYS_MOD: u32 = 4000;

#[derive(Clone, Copy, Debug, Default)]
pub struct Gmod;

impl NavDialect for Gmod {
    fn sub_version(&self) -> u32 {
        0
    }

    fn area_tail_len(&self) -> usize {
        0
    }

    fn write_area_tail(&self, _: &NavArea, _: &mut Vec<u8>) {}
}

impl GameSpec for Gmod {
    fn build(&self) -> Build {
        Build::Gmod
    }

    fn dialect(&self) -> &dyn NavDialect {
        self
    }

    fn policy(&self) -> &dyn WorldPolicy {
        &SourcePolicy
    }

    /// Its own content, then TF2's, which GMod mounts for TF2 maps.
    fn content(&self) -> &'static [GameFolder] {
        &[
            GameFolder { app_id: GARRYS_MOD, folder: "garrysmod" },
            GameFolder { app_id: TF2, folder: "tf" },
            GameFolder { app_id: TF2, folder: "hl2" },
        ]
    }
}
