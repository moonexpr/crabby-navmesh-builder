//! A map's static props (the `sprp` game lump) and its embedded pakfile,
//! read through `vformats`, which knows every branch's record layouts.

use vformats::Limits;

use crate::core::geom::{Vec3, angle_matrix};
use crate::core::world::{PropCollision, StaticProp};

use super::BspError;

fn bsp(bytes: &[u8]) -> Result<vformats::bsp::Bsp<'_>, String> {
    vformats::bsp::parse(bytes, &Limits::default()).map_err(|e| e.to_string())
}

/// Every static prop the map places, with its model's path normalized to
/// lowercase and forward slashes, as the engine's file system matches it.
pub fn static_props(bytes: &[u8]) -> Result<Vec<StaticProp>, BspError> {
    let bsp = bsp(bytes).map_err(BspError::StaticProps)?;
    let Some(lump) = bsp.static_props(&Limits::default()).map_err(|e| BspError::StaticProps(e.to_string()))? else {
        return Ok(Vec::new());
    };
    lump.props
        .iter()
        .enumerate()
        .map(|(index, prop)| {
            let model = lump.models.get(prop.model_index as usize).ok_or_else(|| {
                BspError::StaticProps(format!("prop {index} names model {}, past the dictionary", prop.model_index))
            })?;
            Ok(StaticProp {
                model: normalize_path(model),
                origin: Vec3::from_array(prop.origin),
                rotation: angle_matrix(prop.angles),
                collision: PropCollision::from_solid_type(prop.solid),
            })
        })
        .collect()
}

/// The pakfile's files whose normalized paths `wanted` accepts, as
/// `(normalized path, bytes)`.
pub fn pakfile_files(bytes: &[u8], wanted: impl Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>, BspError> {
    let bsp = bsp(bytes).map_err(BspError::Pakfile)?;
    if bsp.lump(PAKFILE_LUMP).is_none_or(<[u8]>::is_empty) {
        return Ok(Vec::new());
    }
    let zip = bsp.pakfile().map_err(|e| BspError::Pakfile(e.to_string()))?;
    let limits = Limits::default();
    zip.entries()
        .iter()
        .map(|entry| (normalize_path(&entry.path), entry))
        .filter(|(path, _)| wanted(path))
        .map(|(path, entry)| {
            let data = zip.entry_bytes(entry, &limits).map_err(|e| BspError::Pakfile(format!("{path}: {e}")))?;
            Ok((path, data.into_owned()))
        })
        .collect()
}

/// `LUMP_PAKFILE`.
const PAKFILE_LUMP: usize = 40;

/// How the engine's file system compares paths: lowercase, forward slashes.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_without_case_or_backslashes() {
        assert_eq!(normalize_path("Models\\Props_Farm/Barrel.MDL"), "models/props_farm/barrel.mdl");
    }
}
