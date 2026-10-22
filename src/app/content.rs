//! Where a game's files come from, in the engine's order: the map's own
//! pakfile, then each game directory's loose files, then its VPKs. Paths
//! are compared as the engine compares them (see `format::bsp::normalize_path`).

use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use vformats::vpk::VpkLocation;

use anyhow::Context;

use crate::format::bsp::{self, normalize_path};

use super::Failure;

/// A source of game files by path.
pub trait Content: fmt::Debug + Send + Sync {
    /// The file's bytes; `None` when this source lacks it, an error when it
    /// has the file but cannot read it.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ContentError>;
}

/// Files under a game directory, such as `tf/`.
#[derive(Debug)]
pub struct Loose {
    root: PathBuf,
}

impl Loose {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Content for Loose {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ContentError> {
        let file = self.root.join(normalize_path(path));
        match std::fs::read(&file) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ContentError::Io { path: file, source }),
        }
    }
}

/// One VPK: a `*_dir.vpk` and its numbered sibling archives.
#[derive(Debug)]
pub struct Vpk {
    dir_path: PathBuf,
    dir_bytes: Vec<u8>,
    entries: HashMap<String, VpkFile>,
}

#[derive(Debug)]
struct VpkFile {
    preload: Vec<u8>,
    location: VpkLocation,
}

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("reading {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path} is not a VPK directory: {reason}")]
    Vpk { path: PathBuf, reason: String },
}

impl Vpk {
    pub fn open(dir_path: &Path) -> Result<Self, ContentError> {
        let dir_bytes =
            std::fs::read(dir_path).map_err(|source| ContentError::Io { path: dir_path.to_path_buf(), source })?;
        let entries = {
            let directory = vformats::vpk::parse(&dir_bytes, &vformats::Limits::default())
                .map_err(|e| ContentError::Vpk { path: dir_path.to_path_buf(), reason: e.to_string() })?;
            directory
                .entries()
                .iter()
                .map(|e| (normalize_path(&e.path), VpkFile { preload: e.preload.to_vec(), location: e.location }))
                .collect()
        };
        Ok(Self { dir_path: dir_path.to_path_buf(), dir_bytes, entries })
    }

    fn archive_bytes(&self, archive: u16, offset: u64, len: u64) -> Result<Vec<u8>, ContentError> {
        let name = self.dir_path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let archive_name = vformats::vpk::sibling_archive_name(name, archive).ok_or_else(|| ContentError::Vpk {
            path: self.dir_path.clone(),
            reason: format!("archive {archive} has no file name"),
        })?;
        let path = self.dir_path.with_file_name(archive_name);
        let io = |source| ContentError::Io { path: path.clone(), source };
        let mut file = std::fs::File::open(&path).map_err(io)?;
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        let mut chunk = vec![0u8; len as usize];
        file.read_exact(&mut chunk).map_err(io)?;
        Ok(chunk)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Content for Vpk {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ContentError> {
        let Some(file) = self.entries.get(&normalize_path(path)) else {
            return Ok(None);
        };
        let chunk = match file.location {
            VpkLocation::InDirectory { offset, len } => {
                let range = offset as usize..(offset + len) as usize;
                self.dir_bytes.get(range).map(<[u8]>::to_vec).ok_or_else(|| ContentError::Vpk {
                    path: self.dir_path.clone(),
                    reason: format!("{path} lies past the end of the directory file"),
                })?
            }
            VpkLocation::InArchive { archive, offset, len } => self.archive_bytes(archive, offset, len)?,
        };
        let mut bytes = file.preload.clone();
        bytes.extend_from_slice(&chunk);
        Ok(Some(bytes))
    }
}

/// Files the map carries in its own pakfile.
#[derive(Debug, Default)]
pub struct Pakfile {
    files: HashMap<String, Vec<u8>>,
}

impl Pakfile {
    pub fn new(files: Vec<(String, Vec<u8>)>) -> Self {
        Self { files: files.into_iter().collect() }
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Content for Pakfile {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ContentError> {
        Ok(self.files.get(&normalize_path(path)).cloned())
    }
}

/// Sources tried in order; the first that has a file wins.
#[derive(Debug, Default)]
pub struct SearchPath {
    sources: Vec<Box<dyn Content>>,
}

impl SearchPath {
    pub fn push(&mut self, source: Box<dyn Content>) {
        self.sources.push(source);
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

impl Content for SearchPath {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ContentError> {
        for source in &self.sources {
            if let Some(bytes) = source.read(path)? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }
}

/// A game directory's sources: its loose files, then each `*_dir.vpk`
/// directly inside it, by name.
pub fn mount_game_dir(dir: &Path) -> Result<Vec<Box<dyn Content>>, ContentError> {
    let listing = std::fs::read_dir(dir).map_err(|source| ContentError::Io { path: dir.to_path_buf(), source })?;
    let mut vpks: Vec<PathBuf> = listing
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase().ends_with("_dir.vpk"))
        })
        .collect();
    vpks.sort();
    let mut sources: Vec<Box<dyn Content>> = vec![Box::new(Loose::new(dir.to_path_buf()))];
    for vpk in vpks {
        sources.push(Box::new(Vpk::open(&vpk)?));
    }
    Ok(sources)
}

/// The sources prop models are looked for in: the map's own pakfile, then
/// each game directory's loose files and VPKs. Also says what each source is.
pub fn game_content(
    bsp_bytes: &[u8],
    bsp_path: &Path,
    game_dirs: &[PathBuf],
) -> Result<(SearchPath, Vec<String>), Failure> {
    let mut search = SearchPath::default();
    let mut described = Vec::new();
    let packed = bsp::pakfile_files(bsp_bytes, |path| path.ends_with(".mdl") || path.ends_with(".phy"))
        .with_context(|| format!("reading {}'s pakfile", bsp_path.display()))
        .map_err(Failure::input)?;
    let pakfile = Pakfile::new(packed);
    described.push(format!("pakfile ({} model files)", pakfile.len()));
    search.push(Box::new(pakfile));

    let dirs = if game_dirs.is_empty() { detected_game_dirs(bsp_path) } else { game_dirs.to_vec() };
    for dir in dirs {
        let sources = mount_game_dir(&dir).map_err(Failure::input)?;
        described.push(format!("{} (loose files and {} VPKs)", dir.display(), sources.len() - 1));
        sources.into_iter().for_each(|s| search.push(s));
    }
    Ok((search, described))
}

/// For a map at `<game>/maps/<map>.bsp`: `<game>`, and an `hl2` beside it,
/// which the Source games built on Half-Life 2 mount for shared models.
pub fn detected_game_dirs(bsp_path: &Path) -> Vec<PathBuf> {
    let Some(maps) = bsp_path.parent().filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("maps"))) else {
        return Vec::new();
    };
    let Some(game) = maps.parent() else {
        return Vec::new();
    };
    let mut dirs = vec![game.to_path_buf()];
    let hl2 = game.with_file_name("hl2");
    if hl2.is_dir() && hl2 != game {
        dirs.push(hl2);
    }
    dirs
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Debug)]
    struct One(&'static str, &'static [u8]);

    impl Content for One {
        fn read(&self, path: &str) -> Result<Option<Vec<u8>>, ContentError> {
            Ok((path == self.0).then(|| self.1.to_vec()))
        }
    }

    /// A version 1 VPK directory holding `files`, each stored right after the tree.
    pub(crate) fn vpk_v1(files: &[(&str, &str, &str, &[u8])]) -> Vec<u8> {
        let mut tree = Vec::new();
        let mut data = Vec::new();
        for (ext, dir, name, bytes) in files {
            for part in [*ext, *dir, *name] {
                tree.extend_from_slice(part.as_bytes());
                tree.push(0);
            }
            tree.extend_from_slice(&vformats::crc32_ieee(bytes).to_le_bytes());
            tree.extend_from_slice(&0u16.to_le_bytes()); // preload bytes
            tree.extend_from_slice(&0x7fffu16.to_le_bytes()); // stored after the directory
            tree.extend_from_slice(&(data.len() as u32).to_le_bytes());
            tree.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            tree.extend_from_slice(&0xffffu16.to_le_bytes());
            tree.extend_from_slice(&[0, 0]); // end of names, end of directories
            data.extend_from_slice(bytes);
        }
        tree.push(0); // end of extensions
        let mut out = Vec::new();
        out.extend_from_slice(&0x55aa_1234u32.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(tree.len() as u32).to_le_bytes());
        out.extend_from_slice(&tree);
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn the_first_source_with_a_file_wins() {
        let mut path = SearchPath::default();
        path.push(Box::new(One("models/a.mdl", b"pakfile")));
        path.push(Box::new(One("models/a.mdl", b"loose")));
        path.push(Box::new(One("models/b.mdl", b"vpk")));
        assert_eq!(path.read("models/a.mdl").expect("readable").as_deref(), Some(&b"pakfile"[..]));
        assert_eq!(path.read("models/b.mdl").expect("readable").as_deref(), Some(&b"vpk"[..]));
        assert_eq!(path.read("models/c.mdl").expect("readable"), None);
    }

    #[test]
    fn loose_files_are_found_under_their_game_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("models/props")).expect("dirs");
        std::fs::write(dir.path().join("models/props/crate.phy"), b"phy").expect("written");
        let loose = Loose::new(dir.path().to_path_buf());
        assert_eq!(loose.read("models/props/crate.phy").expect("readable").as_deref(), Some(&b"phy"[..]));
        assert_eq!(loose.read("models/props/missing.phy").expect("readable"), None);
    }

    #[test]
    fn a_vpk_reads_its_entries_by_normalized_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test_dir.vpk");
        std::fs::write(&path, vpk_v1(&[("mdl", "models/props", "barrel", b"barrel bytes")])).expect("written");
        let vpk = Vpk::open(&path).expect("opens");
        assert_eq!(vpk.len(), 1);
        assert_eq!(vpk.read("Models/Props/Barrel.MDL").expect("readable").as_deref(), Some(&b"barrel bytes"[..]));
        assert_eq!(vpk.read("models/props/crate.mdl").expect("readable"), None);
    }

    #[test]
    fn a_file_that_is_not_a_vpk_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("broken_dir.vpk");
        std::fs::write(&path, b"nope").expect("written");
        assert!(matches!(Vpk::open(&path), Err(ContentError::Vpk { .. })));
    }

    #[test]
    fn mounting_a_game_dir_takes_its_loose_files_then_its_vpks() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("pak_dir.vpk"), vpk_v1(&[("phy", "models", "rock", b"from vpk")]))
            .expect("written");
        std::fs::write(dir.path().join("pak_000.vpk"), b"an archive, not a directory").expect("written");
        std::fs::create_dir_all(dir.path().join("models")).expect("dirs");
        std::fs::write(dir.path().join("models/rock.phy"), b"loose wins").expect("written");
        let sources = mount_game_dir(dir.path()).expect("mounts");
        assert_eq!(sources.len(), 2, "the loose directory and the one _dir.vpk");
        let mut path = SearchPath::default();
        sources.into_iter().for_each(|s| path.push(s));
        assert_eq!(path.read("models/rock.phy").expect("readable").as_deref(), Some(&b"loose wins"[..]));
    }
}
