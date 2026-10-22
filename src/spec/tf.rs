//! Team Fortress 2: `CTFNavMesh` writes sub-version 2, and `CTFNavArea::Save`
//! appends a u32 of TF attributes to every area.

use crate::core::mesh::NavArea;
use crate::core::world::WorldPolicy;
use crate::format::nav::NavDialect;

use super::{Build, GameFolder, GameSpec, SourcePolicy};

/// Team Fortress 2's Steam app id.
pub const TF2: u32 = 440;

/// TF attributes a generated mesh starts with; bots' spawn rooms and the
/// like are marked in game.
const NO_TF_ATTRIBUTES: u32 = 0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Tf;

impl NavDialect for Tf {
    fn sub_version(&self) -> u32 {
        2
    }

    fn area_tail_len(&self) -> usize {
        4
    }

    fn write_area_tail(&self, _: &NavArea, out: &mut Vec<u8>) {
        out.extend_from_slice(&NO_TF_ATTRIBUTES.to_le_bytes());
    }
}

impl GameSpec for Tf {
    fn build(&self) -> Build {
        Build::Tf
    }

    fn dialect(&self) -> &dyn NavDialect {
        self
    }

    fn policy(&self) -> &dyn WorldPolicy {
        &SourcePolicy
    }

    fn content(&self) -> &'static [GameFolder] {
        &[GameFolder { app_id: TF2, folder: "tf" }, GameFolder { app_id: TF2, folder: "hl2" }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mesh::{AreaId, Attributes, CornerHeights};

    #[test]
    fn each_area_ends_with_a_u32_of_tf_attributes() {
        let flat = CornerHeights { north_west: 0.0, north_east: 0.0, south_east: 0.0, south_west: 0.0 };
        let id = AreaId::new(1).expect("nonzero");
        let area =
            NavArea::new(id, Attributes::NONE, (0.0, 0.0), (25.0, 25.0), flat, Default::default()).expect("valid");
        let mut out = Vec::new();
        Tf.write_area_tail(&area, &mut out);
        assert_eq!(out, vec![0, 0, 0, 0]);
        assert_eq!(Tf.area_tail_len(), out.len());
    }
}
