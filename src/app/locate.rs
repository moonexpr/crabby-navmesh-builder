//! Finding a game's installed content when the map does not say where it
//! is. In order: the Steam home cached by an earlier run; Steam's own
//! registry entry and each platform's default install paths; Source
//! launchers that are running or on `PATH`, traced back to the install they
//! run from. Of the Steam installs found, the one holding the most of the
//! wanted content wins and is cached. When none holds any, the caller asks.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::Failure;
use crate::format::steam::{app_install_dir, library_folders};
use crate::spec::GameFolder;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Other,
}

/// The machine, as far as finding games needs it.
pub trait Environment {
    fn platform(&self) -> Platform;
    fn home(&self) -> Option<PathBuf>;
    /// Steam's own `HKCU\Software\Valve\Steam\SteamPath`, on Windows.
    fn steam_registry_home(&self) -> Option<PathBuf>;
    /// The executables of running processes, and of commands on `PATH`,
    /// that are named like a Source engine launcher.
    fn launchers(&self) -> Vec<PathBuf>;
    fn is_dir(&self, path: &Path) -> bool;
    fn is_file(&self, path: &Path) -> bool;
    fn read_to_string(&self, path: &Path) -> Option<String>;
    /// Entries of a directory, sorted; empty when it cannot be read.
    fn list_dir(&self, path: &Path) -> Vec<PathBuf>;
    fn read_link(&self, path: &Path) -> Option<PathBuf>;
}

/// Where the Steam home found last time is kept.
pub trait HomeCache {
    fn load(&self) -> Option<PathBuf>;
    fn store(&self, home: &Path) -> Result<(), String>;
}

/// Executable names of the Steam client, Source games, and dedicated servers.
pub const LAUNCHERS: [&str; 16] = [
    "steam",
    "steam.exe",
    "steam.sh",
    "hl2.exe",
    "hl2_linux",
    "hl2_osx",
    "hl2.sh",
    "tf.exe",
    "tf_win64.exe",
    "tf_linux64",
    "gmod.exe",
    "srcds.exe",
    "srcds_win64.exe",
    "srcds_linux",
    "srcds_linux64",
    "srcds_run",
];

/// How the content was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundBy {
    Cache,
    SteamRegistry,
    DefaultPath,
    Launcher,
    /// The user typed it in when nothing else found it.
    Asked,
}

/// What became of the cache.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheOutcome {
    /// The cached home was used as it was.
    Kept,
    /// A newly found home was cached.
    Stored,
    /// The content was found outside any Steam install; nothing to cache.
    NotApplicable,
    /// Caching failed; the next run looks again.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Located {
    /// Game content folders, most specific first.
    pub dirs: Vec<PathBuf>,
    pub steam_home: Option<PathBuf>,
    pub found_by: FoundBy,
    pub cache: CacheOutcome,
}

/// Where game content comes from when nothing names it: the machine
/// searched, then the user asked.
pub trait GameLocator {
    fn locate(&self, games: &[GameFolder]) -> Result<Located, Failure>;
}

/// The game folders to read prop models from. Named ones (`--game-dir`)
/// win; then the game the map sits in; then, only if a solid prop needs
/// a model, whatever `locator` finds. Says how they were found when the
/// locator was asked.
pub fn choose_game_dirs(
    named: &[PathBuf],
    beside_map: Vec<PathBuf>,
    needs_models: bool,
    wanted: &[GameFolder],
    locator: &dyn GameLocator,
) -> Result<(Vec<PathBuf>, Option<Located>), Failure> {
    if !named.is_empty() {
        return Ok((named.to_vec(), None));
    }
    if !beside_map.is_empty() || !needs_models {
        return Ok((beside_map, None));
    }
    let located = locator.locate(wanted)?;
    Ok((located.dirs.clone(), Some(located)))
}

/// Nothing on the machine held the wanted content.
#[derive(Clone, Debug, thiserror::Error, PartialEq)]
pub struct GameNotFound {
    pub wanted: Vec<String>,
    pub tried: Vec<PathBuf>,
}

impl fmt::Display for GameNotFound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "found no installed {} content after looking in {} places; pass --game-dir with the game's content folder (such as .../Team Fortress 2/tf)",
            self.wanted.join(", "),
            self.tried.len()
        )
    }
}

pub fn locate(games: &[GameFolder], env: &dyn Environment, cache: &dyn HomeCache) -> Result<Located, GameNotFound> {
    let mut tried = Vec::new();
    let mut best: Option<(Candidate, Vec<PathBuf>)> = None;
    let mut consider = |candidate: Candidate, tried: &mut Vec<PathBuf>| {
        tried.push(candidate.root().to_path_buf());
        let dirs = candidate.resolve(games, env);
        let better = best.as_ref().is_none_or(|(_, held)| dirs.len() > held.len());
        if !dirs.is_empty() && better {
            best = Some((candidate, dirs));
        }
        best.as_ref().is_some_and(|(_, held)| held.len() == games.len())
    };

    // a cached home that still holds everything ends the search at once
    if let Some(home) = cache.load()
        && consider(Candidate::Steam { home, by: FoundBy::Cache }, &mut tried)
    {
        return Ok(finish(best.expect("just found"), cache));
    }
    for candidate in search_order(env) {
        if consider(candidate, &mut tried) {
            break;
        }
    }
    match best {
        Some(found) => Ok(finish(found, cache)),
        None => Err(GameNotFound { wanted: games.iter().map(|g| g.folder.to_string()).collect(), tried }),
    }
}

/// Somewhere game content might be.
#[derive(Clone, Debug, PartialEq)]
enum Candidate {
    /// A Steam install: a folder holding `steamapps`.
    Steam { home: PathBuf, by: FoundBy },
    /// A game installed outside Steam, such as a dedicated server: a folder
    /// holding the game folders themselves.
    Install { root: PathBuf },
}

impl Candidate {
    fn root(&self) -> &Path {
        match self {
            Candidate::Steam { home, .. } => home,
            Candidate::Install { root } => root,
        }
    }

    /// The wanted folders this candidate holds, in the order wanted.
    fn resolve(&self, games: &[GameFolder], env: &dyn Environment) -> Vec<PathBuf> {
        match self {
            Candidate::Install { root } => {
                games.iter().map(|g| root.join(g.folder)).filter(|dir| env.is_file(&dir.join("gameinfo.txt"))).collect()
            }
            Candidate::Steam { home, .. } => {
                if !env.is_dir(&home.join("steamapps")) {
                    return Vec::new();
                }
                let libraries = libraries(home, env);
                games.iter().filter_map(|game| installed(game, &libraries, env)).collect()
            }
        }
    }
}

/// A Steam home's libraries: itself, then those `libraryfolders.vdf` lists.
fn libraries(home: &Path, env: &dyn Environment) -> Vec<PathBuf> {
    let mut found = vec![home.to_path_buf()];
    let listed = env
        .read_to_string(&home.join("steamapps").join("libraryfolders.vdf"))
        .and_then(|text| library_folders(&text).ok())
        .unwrap_or_default();
    for path in listed {
        if let Some(path) = host_path(&path, home, env.platform())
            && !found.contains(&path)
        {
            found.push(path);
        }
    }
    found
}

/// A path as Steam wrote it, as this machine reaches it. Steam doubles its
/// backslashes; a Steam running under Wine writes Windows paths, which live
/// under its bottle's `drive_c`, or `dosdevices` for other drives.
fn host_path(written: &str, home: &Path, platform: Platform) -> Option<PathBuf> {
    let path = written.replace("\\\\", "\\");
    let bytes = path.as_bytes();
    let is_windows_path = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if !is_windows_path {
        return Some(PathBuf::from(path));
    }
    let bottle = home.ancestors().find(|a| a.file_name().is_some_and(|n| n == "drive_c")).and_then(Path::parent);
    match bottle {
        Some(bottle) => {
            let drive = bytes[0].to_ascii_lowercase() as char;
            let mut mapped =
                if drive == 'c' { bottle.join("drive_c") } else { bottle.join("dosdevices").join(format!("{drive}:")) };
            for part in path[2..].split(['\\', '/']).filter(|p| !p.is_empty()) {
                mapped.push(part);
            }
            Some(mapped)
        }
        None if platform == Platform::Windows => Some(PathBuf::from(path)),
        None => None,
    }
}

/// Where a game's folder is, in the first library whose manifest names it.
fn installed(game: &GameFolder, libraries: &[PathBuf], env: &dyn Environment) -> Option<PathBuf> {
    libraries.iter().find_map(|library| {
        let manifest = library.join("steamapps").join(format!("appmanifest_{}.acf", game.app_id));
        let install_dir = env.read_to_string(&manifest).and_then(|text| app_install_dir(&text).ok().flatten())?;
        let dir = library.join("steamapps").join("common").join(install_dir).join(game.folder);
        env.is_dir(&dir).then_some(dir)
    })
}

/// Steam's registry entry, then each platform's default homes, then
/// launchers traced back, each place once.
fn search_order(env: &dyn Environment) -> Vec<Candidate> {
    let mut order = Vec::new();
    if let Some(home) = env.steam_registry_home() {
        order.push(Candidate::Steam { home, by: FoundBy::SteamRegistry });
    }
    order.extend(default_homes(env).into_iter().map(|home| Candidate::Steam { home, by: FoundBy::DefaultPath }));
    for launcher in env.launchers() {
        let executable = env.read_link(&launcher).unwrap_or(launcher);
        order.extend(trace_back(&executable, env));
    }
    let mut seen = BTreeSet::new();
    order.retain(|c| seen.insert(c.root().to_path_buf()));
    order
}

fn default_homes(env: &dyn Environment) -> Vec<PathBuf> {
    let home = env.home();
    let under = |parts: &[&str]| home.as_ref().map(|h| parts.iter().fold(h.clone(), |p, part| p.join(part)));
    match env.platform() {
        Platform::Windows => {
            vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"C:\Program Files\Steam")]
        }
        Platform::MacOs => {
            let mut homes: Vec<PathBuf> = under(&["Library", "Application Support", "Steam"]).into_iter().collect();
            if let Some(bottles) = under(&["Library", "Application Support", "CrossOver", "Bottles"]) {
                for bottle in env.list_dir(&bottles) {
                    for program_files in ["Program Files (x86)", "Program Files"] {
                        homes.push(bottle.join("drive_c").join(program_files).join("Steam"));
                    }
                }
            }
            homes
        }
        Platform::Linux | Platform::Other => [
            &[".steam", "steam"][..],
            &[".steam", "root"],
            &[".local", "share", "Steam"],
            &[".var", "app", "com.valvesoftware.Steam", ".local", "share", "Steam"],
            &["snap", "steam", "common", ".local", "share", "Steam"],
            &[".wine", "drive_c", "Program Files (x86)", "Steam"],
        ]
        .iter()
        .filter_map(|parts| under(parts))
        .collect(),
    }
}

/// The install a launcher runs from: the nearest folder above it that is a
/// Steam home, or that holds a game folder with a `gameinfo.txt`.
fn trace_back(executable: &Path, env: &dyn Environment) -> Option<Candidate> {
    executable.ancestors().skip(1).find_map(|folder| {
        if env.is_dir(&folder.join("steamapps")) {
            return Some(Candidate::Steam { home: folder.to_path_buf(), by: FoundBy::Launcher });
        }
        let holds_a_game = env.list_dir(folder).iter().any(|child| env.is_file(&child.join("gameinfo.txt")));
        holds_a_game.then(|| Candidate::Install { root: folder.to_path_buf() })
    })
}

fn finish((candidate, dirs): (Candidate, Vec<PathBuf>), cache: &dyn HomeCache) -> Located {
    match candidate {
        Candidate::Install { .. } => {
            Located { dirs, steam_home: None, found_by: FoundBy::Launcher, cache: CacheOutcome::NotApplicable }
        }
        Candidate::Steam { home, by } => {
            let outcome = match by {
                FoundBy::Cache => CacheOutcome::Kept,
                _ => match cache.store(&home) {
                    Ok(()) => CacheOutcome::Stored,
                    Err(reason) => CacheOutcome::Failed(reason),
                },
            };
            Located { dirs, steam_home: Some(home), found_by: by, cache: outcome }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    const TF: GameFolder = GameFolder { app_id: 440, folder: "tf" };
    const HL2: GameFolder = GameFolder { app_id: 440, folder: "hl2" };
    const GMOD: GameFolder = GameFolder { app_id: 4000, folder: "garrysmod" };

    /// A machine made of the files and directories a test puts on it.
    pub(crate) struct Machine {
        pub platform: Platform,
        pub home: Option<PathBuf>,
        pub registry: Option<PathBuf>,
        pub launchers: Vec<PathBuf>,
        pub files: BTreeMap<PathBuf, String>,
        pub dirs: BTreeSet<PathBuf>,
        pub links: BTreeMap<PathBuf, PathBuf>,
    }

    impl Machine {
        pub(crate) fn new(platform: Platform, home: &str) -> Self {
            Self {
                platform,
                home: Some(PathBuf::from(home)),
                registry: None,
                launchers: vec![],
                files: BTreeMap::new(),
                dirs: BTreeSet::new(),
                links: BTreeMap::new(),
            }
        }

        pub(crate) fn dir(&mut self, path: impl AsRef<Path>) -> &mut Self {
            for ancestor in path.as_ref().ancestors() {
                self.dirs.insert(ancestor.to_path_buf());
            }
            self
        }

        pub(crate) fn file(&mut self, path: impl AsRef<Path>, text: &str) -> &mut Self {
            if let Some(parent) = path.as_ref().parent() {
                self.dir(parent);
            }
            self.files.insert(path.as_ref().to_path_buf(), text.to_string());
            self
        }

        /// A Steam install at `home` with these games, each in `steamapps/common/<install dir>`.
        pub(crate) fn steam(&mut self, home: &str, games: &[(u32, &str, &[&str])]) -> &mut Self {
            let home = PathBuf::from(home);
            self.dir(home.join("steamapps"));
            for (app_id, install_dir, folders) in games {
                let manifest = format!("\"AppState\" {{ \"appid\" \"{app_id}\" \"installdir\" \"{install_dir}\" }}");
                self.file(home.join(format!("steamapps/appmanifest_{app_id}.acf")), &manifest);
                for folder in *folders {
                    self.dir(home.join("steamapps/common").join(install_dir).join(folder));
                }
            }
            self
        }
    }

    impl Environment for Machine {
        fn platform(&self) -> Platform {
            self.platform
        }
        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
        fn steam_registry_home(&self) -> Option<PathBuf> {
            self.registry.clone()
        }
        fn launchers(&self) -> Vec<PathBuf> {
            self.launchers.clone()
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains(path)
        }
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }
        fn read_to_string(&self, path: &Path) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
            self.dirs.iter().filter(|d| d.parent() == Some(path)).cloned().collect()
        }
        fn read_link(&self, path: &Path) -> Option<PathBuf> {
            self.links.get(path).cloned()
        }
    }

    #[derive(Default)]
    pub(crate) struct Remembered {
        pub home: RefCell<Option<PathBuf>>,
        pub stores: RefCell<usize>,
    }

    impl HomeCache for Remembered {
        fn load(&self) -> Option<PathBuf> {
            self.home.borrow().clone()
        }
        fn store(&self, home: &Path) -> Result<(), String> {
            *self.home.borrow_mut() = Some(home.to_path_buf());
            *self.stores.borrow_mut() += 1;
            Ok(())
        }
    }

    const LINUX_STEAM: &str = "/home/p/.local/share/Steam";

    fn tf_install(root: &str) -> Vec<PathBuf> {
        vec![
            PathBuf::from(format!("{root}/steamapps/common/Team Fortress 2/tf")),
            PathBuf::from(format!("{root}/steamapps/common/Team Fortress 2/hl2")),
        ]
    }

    #[test]
    fn a_default_install_path_is_found_and_cached() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.steam(LINUX_STEAM, &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        let cache = Remembered::default();
        let located = locate(&[TF, HL2], &machine, &cache).expect("found");
        assert_eq!(located.dirs, tf_install(LINUX_STEAM));
        assert_eq!((located.found_by, located.cache), (FoundBy::DefaultPath, CacheOutcome::Stored));
        assert_eq!(cache.load(), Some(PathBuf::from(LINUX_STEAM)));
    }

    #[test]
    fn a_cached_home_that_still_holds_everything_is_used_as_is() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.steam("/games/Steam", &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        let cache = Remembered::default();
        cache.store(Path::new("/games/Steam")).expect("stored");
        let located = locate(&[TF, HL2], &machine, &cache).expect("found");
        assert_eq!((located.found_by, located.cache), (FoundBy::Cache, CacheOutcome::Kept));
        assert_eq!(*cache.stores.borrow(), 1, "not written again");
    }

    #[test]
    fn a_stale_cache_gives_way_to_a_fresh_search() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.steam(LINUX_STEAM, &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        let cache = Remembered::default();
        cache.store(Path::new("/gone/Steam")).expect("stored");
        let located = locate(&[TF, HL2], &machine, &cache).expect("found");
        assert_eq!(located.found_by, FoundBy::DefaultPath);
        assert_eq!(cache.load(), Some(PathBuf::from(LINUX_STEAM)));
    }

    #[test]
    fn on_windows_steams_registry_entry_comes_before_program_files() {
        let mut machine = Machine::new(Platform::Windows, r"C:\Users\p");
        machine.steam(r"D:\Steam", &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        machine.steam(r"C:\Program Files (x86)\Steam", &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        machine.registry = Some(PathBuf::from(r"D:\Steam"));
        let located = locate(&[TF, HL2], &machine, &Remembered::default()).expect("found");
        assert_eq!(located.found_by, FoundBy::SteamRegistry);
        assert_eq!(located.steam_home, Some(PathBuf::from(r"D:\Steam")));
    }

    #[test]
    fn games_in_another_library_are_found_through_libraryfolders() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.steam(LINUX_STEAM, &[]);
        machine.file(
            format!("{LINUX_STEAM}/steamapps/libraryfolders.vdf"),
            r#""libraryfolders" { "0" { "path" "/home/p/.local/share/Steam" } "1" { "path" "/mnt/ssd/SteamLibrary" } }"#,
        );
        machine.steam("/mnt/ssd/SteamLibrary", &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        let located = locate(&[TF, HL2], &machine, &Remembered::default()).expect("found");
        assert_eq!(located.dirs, tf_install("/mnt/ssd/SteamLibrary"));
        assert_eq!(located.steam_home, Some(PathBuf::from(LINUX_STEAM)));
    }

    #[test]
    fn a_crossover_bottles_windows_paths_are_read_through_its_drive_c() {
        let bottle = "/Users/p/Library/Application Support/CrossOver/Bottles/Steam";
        let steam = format!("{bottle}/drive_c/Program Files (x86)/Steam");
        let mut machine = Machine::new(Platform::MacOs, "/Users/p");
        machine.steam(&steam, &[(440, "Team Fortress 2", &["tf", "hl2"]), (4000, "GarrysMod", &["garrysmod"])]);
        machine.file(
            format!("{steam}/steamapps/libraryfolders.vdf"),
            r#""libraryfolders" { "0" { "path" "C:\\Program Files (x86)\\Steam" } }"#,
        );
        // a native Steam holding only Garry's Mod loses to the bottle holding more
        machine.steam("/Users/p/Library/Application Support/Steam", &[(4000, "GarrysMod", &["garrysmod"])]);
        let located = locate(&[GMOD, TF, HL2], &machine, &Remembered::default()).expect("found");
        assert_eq!(located.steam_home, Some(PathBuf::from(&steam)));
        assert_eq!(located.dirs.len(), 3);
        assert_eq!(located.dirs[0], PathBuf::from(format!("{steam}/steamapps/common/GarrysMod/garrysmod")));
    }

    #[test]
    fn a_running_launcher_leads_back_to_its_steam_install() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.steam("/opt/Steam", &[(440, "Team Fortress 2", &["tf", "hl2"])]);
        machine.launchers = vec![PathBuf::from("/opt/Steam/steamapps/common/Team Fortress 2/tf_linux64")];
        let located = locate(&[TF, HL2], &machine, &Remembered::default()).expect("found");
        assert_eq!((located.found_by, located.steam_home), (FoundBy::Launcher, Some(PathBuf::from("/opt/Steam"))));
    }

    #[test]
    fn a_dedicated_server_outside_steam_is_used_directly() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.file("/srv/tf2/tf/gameinfo.txt", "\"GameInfo\" {}");
        machine.file("/srv/tf2/hl2/gameinfo.txt", "\"GameInfo\" {}");
        machine.launchers = vec![PathBuf::from("/srv/tf2/srcds_linux64")];
        let cache = Remembered::default();
        let located = locate(&[TF, HL2], &machine, &cache).expect("found");
        assert_eq!(located.dirs, vec![PathBuf::from("/srv/tf2/tf"), PathBuf::from("/srv/tf2/hl2")]);
        assert_eq!((located.steam_home, located.cache), (None, CacheOutcome::NotApplicable));
        assert_eq!(cache.load(), None);
    }

    /// Finds TF2 at a fixed place, counting how often it is asked.
    struct Fixed(RefCell<usize>);

    impl GameLocator for Fixed {
        fn locate(&self, _: &[GameFolder]) -> Result<Located, Failure> {
            *self.0.borrow_mut() += 1;
            Ok(Located {
                dirs: tf_install("/found"),
                steam_home: Some(PathBuf::from("/found")),
                found_by: FoundBy::DefaultPath,
                cache: CacheOutcome::Stored,
            })
        }
    }

    #[test]
    fn named_game_dirs_win_and_nothing_is_searched() {
        let locator = Fixed(RefCell::new(0));
        let (dirs, located) =
            choose_game_dirs(&[PathBuf::from("/mine/tf")], tf_install("/beside"), true, &[TF], &locator).expect("ok");
        assert_eq!((dirs, located, *locator.0.borrow()), (vec![PathBuf::from("/mine/tf")], None, 0));
    }

    #[test]
    fn the_game_the_map_sits_in_comes_next() {
        let locator = Fixed(RefCell::new(0));
        let (dirs, _) = choose_game_dirs(&[], tf_install("/beside"), true, &[TF], &locator).expect("ok");
        assert_eq!((dirs, *locator.0.borrow()), (tf_install("/beside"), 0));
    }

    #[test]
    fn the_machine_is_searched_only_when_a_solid_prop_needs_a_model() {
        let locator = Fixed(RefCell::new(0));
        let (dirs, _) = choose_game_dirs(&[], vec![], false, &[TF], &locator).expect("ok");
        assert_eq!((dirs.len(), *locator.0.borrow()), (0, 0));
        let (dirs, located) = choose_game_dirs(&[], vec![], true, &[TF], &locator).expect("ok");
        assert_eq!((dirs, *locator.0.borrow()), (tf_install("/found"), 1));
        assert_eq!(located.map(|l| l.found_by), Some(FoundBy::DefaultPath));
    }

    #[test]
    fn with_nothing_installed_it_gives_up_saying_where_it_looked() {
        let machine = Machine::new(Platform::Linux, "/home/p");
        let error = locate(&[TF, HL2], &machine, &Remembered::default()).expect_err("nothing there");
        assert!(!error.tried.is_empty());
        assert!(error.to_string().contains("--game-dir"));
    }
}
