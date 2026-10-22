//! Steam's own manifests, read through `vformats`' KeyValues parser:
//! `steamapps/libraryfolders.vdf`, which lists a Steam install's libraries,
//! and `steamapps/appmanifest_<id>.acf`, which names a game's folder.

use vformats::Limits;
use vformats::keyvalues::{self, KvDocument};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SteamError {
    #[error("{file}: {reason}")]
    KeyValues { file: &'static str, reason: String },
    #[error("{file} has no {block} block")]
    MissingBlock { file: &'static str, block: &'static str },
}

fn document<'a>(text: &'a str, file: &'static str) -> Result<KvDocument<'a>, SteamError> {
    keyvalues::parse(text, &Limits::default()).map_err(|e| SteamError::KeyValues { file, reason: e.to_string() })
}

/// The library paths `libraryfolders.vdf` lists, exactly as written (so
/// a Steam under Wine gives Windows paths). Both layouts are read: blocks
/// with a `path` key, and the older one of bare paths.
pub fn library_folders(text: &str) -> Result<Vec<String>, SteamError> {
    const FILE: &str = "libraryfolders.vdf";
    let doc = document(text, FILE)?;
    let block = doc
        .get("libraryfolders")
        .and_then(|v| v.as_block())
        .ok_or(SteamError::MissingBlock { file: FILE, block: "libraryfolders" })?;
    // libraries are the numbered entries; the rest are Steam's bookkeeping
    Ok(block
        .pairs
        .iter()
        .filter(|pair| !pair.key.is_empty() && pair.key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|pair| pair.value.as_block().map_or(pair.value.as_str(), |library| library.get_str("path")))
        .map(str::to_string)
        .collect())
}

/// The folder under `steamapps/common` an `appmanifest_<id>.acf` installs into.
pub fn app_install_dir(text: &str) -> Result<Option<String>, SteamError> {
    const FILE: &str = "appmanifest";
    let doc = document(text, FILE)?;
    let state = doc
        .get("AppState")
        .and_then(|v| v.as_block())
        .ok_or(SteamError::MissingBlock { file: FILE, block: "AppState" })?;
    Ok(state.get_str("installdir").map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libraries_are_read_from_their_path_keys() {
        let text = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps" { "440" "33279160746" }
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}"#;
        assert_eq!(
            library_folders(text),
            Ok(vec![r"C:\\Program Files (x86)\\Steam".to_string(), r"D:\\SteamLibrary".to_string()])
        );
    }

    #[test]
    fn the_older_layout_of_bare_paths_is_read_too() {
        let text = r#""LibraryFolders" { "TimeNextStatsReport" "1693000000" "ContentStatsID" "-1" "1" "/mnt/games/SteamLibrary" }"#;
        assert_eq!(library_folders(text), Ok(vec!["/mnt/games/SteamLibrary".to_string()]));
    }

    #[test]
    fn a_manifest_names_its_install_folder() {
        let text = r#""AppState" { "appid" "440" "name" "Team Fortress 2" "installdir" "Team Fortress 2" }"#;
        assert_eq!(app_install_dir(text), Ok(Some("Team Fortress 2".to_string())));
        assert_eq!(app_install_dir(r#""AppState" { "appid" "440" }"#), Ok(None));
    }

    #[test]
    fn a_file_without_its_block_is_refused() {
        assert_eq!(
            library_folders(r#""Other" { }"#),
            Err(SteamError::MissingBlock { file: "libraryfolders.vdf", block: "libraryfolders" })
        );
    }
}
