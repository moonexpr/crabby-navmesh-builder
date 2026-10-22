//! The machine itself, behind `locate`'s ports: files, processes, `PATH`,
//! Steam's registry entry, and a cache of the Steam home found last time in
//! each platform's own settings store (the registry on Windows, user
//! defaults on macOS, an XDG config file elsewhere).

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::anyhow;

use crate::spec::GameFolder;

use super::Failure;
use super::locate::{CacheOutcome, Environment, FoundBy, GameLocator, HomeCache, LAUNCHERS, Located, Platform, locate};

/// The settings key the cache lives under.
pub const SETTINGS_NAME: &str = "Crabby Navmesh Builder";
/// macOS user defaults domain.
pub const DEFAULTS_DOMAIN: &str = "io.github.moonexpr.crabby-navmesh-builder";
const CACHE_KEY: &str = "SteamHome";

/// This machine.
#[derive(Debug, Default)]
pub struct SystemEnvironment;

impl Environment for SystemEnvironment {
    fn platform(&self) -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
    fn home(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
    fn steam_registry_home(&self) -> Option<PathBuf> {
        windows::steam_path()
    }
    fn launchers(&self) -> Vec<PathBuf> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let is_launcher = |path: &Path| {
            path.file_name().and_then(|n| n.to_str()).is_some_and(|n| LAUNCHERS.contains(&n.to_lowercase().as_str()))
        };
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        let mut found: Vec<PathBuf> = system
            .processes()
            .values()
            .filter_map(|p| p.exe())
            .filter(|exe| is_launcher(exe))
            .map(Path::to_path_buf)
            .collect();
        let on_path =
            std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect::<Vec<_>>()).unwrap_or_default();
        for dir in on_path {
            found.extend(LAUNCHERS.iter().map(|name| dir.join(name)).filter(|candidate| candidate.is_file()));
        }
        found.sort();
        found.dedup();
        found
    }
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }
    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read(path).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }
    fn list_dir(&self, path: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> =
            std::fs::read_dir(path).map(|it| it.filter_map(Result::ok).map(|e| e.path()).collect()).unwrap_or_default();
        entries.sort();
        entries
    }
    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok().filter(|resolved| resolved != path)
    }
}

/// A one-line file holding the home, for platforms without a registry.
#[derive(Debug)]
pub struct FileCache {
    path: PathBuf,
}

impl FileCache {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl HomeCache for FileCache {
    fn load(&self) -> Option<PathBuf> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let line = text.lines().next()?.trim();
        (!line.is_empty()).then(|| PathBuf::from(line))
    }
    fn store(&self, home: &Path) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&self.path, format!("{}\n", home.display()))
            .map_err(|e| format!("writing {}: {e}", self.path.display()))
    }
}

/// This platform's cache: the registry, user defaults, or a config file.
pub fn system_cache() -> Box<dyn HomeCache> {
    if cfg!(windows) {
        return Box::new(windows::RegistryCache);
    }
    if cfg!(target_os = "macos") {
        return Box::new(DefaultsCache);
    }
    let config = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
    Box::new(FileCache::new(config.join("crabby-navmesh-builder").join("steam-home")))
}

/// macOS user defaults, through the `defaults` tool every macOS has.
#[derive(Debug)]
pub struct DefaultsCache;

impl HomeCache for DefaultsCache {
    fn load(&self) -> Option<PathBuf> {
        let output = std::process::Command::new("defaults").args(["read", DEFAULTS_DOMAIN, CACHE_KEY]).output().ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        let line = text.trim();
        (output.status.success() && !line.is_empty()).then(|| PathBuf::from(line))
    }
    fn store(&self, home: &Path) -> Result<(), String> {
        let home = home.to_str().ok_or_else(|| format!("{} is not valid UTF-8", home.display()))?;
        let status = std::process::Command::new("defaults")
            .args(["write", DEFAULTS_DOMAIN, CACHE_KEY, "-string", home])
            .status()
            .map_err(|e| format!("running defaults: {e}"))?;
        status.success().then_some(()).ok_or_else(|| format!("defaults write exited with {status}"))
    }
}

#[cfg(windows)]
mod windows {
    use std::path::{Path, PathBuf};

    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::{CACHE_KEY, HomeCache, SETTINGS_NAME};

    pub fn steam_path() -> Option<PathBuf> {
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
        let path: String = key.get_value("SteamPath").ok()?;
        Some(PathBuf::from(path.replace('/', "\\")))
    }

    /// `HKCU\Software\Crabby Navmesh Builder\SteamHome`.
    #[derive(Debug)]
    pub struct RegistryCache;

    impl HomeCache for RegistryCache {
        fn load(&self) -> Option<PathBuf> {
            let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(r"Software\{SETTINGS_NAME}")).ok()?;
            key.get_value::<String, _>(CACHE_KEY).ok().map(PathBuf::from)
        }
        fn store(&self, home: &Path) -> Result<(), String> {
            let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
                .create_subkey(format!(r"Software\{SETTINGS_NAME}"))
                .map_err(|e| format!("opening the registry key: {e}"))?;
            key.set_value(CACHE_KEY, &home.display().to_string())
                .map_err(|e| format!("writing the registry value: {e}"))
        }
    }
}

#[cfg(not(windows))]
mod windows {
    use std::path::{Path, PathBuf};

    use super::HomeCache;

    pub fn steam_path() -> Option<PathBuf> {
        None
    }

    /// Never built off Windows; `system_cache` picks another store.
    #[derive(Debug)]
    pub struct RegistryCache;

    impl HomeCache for RegistryCache {
        fn load(&self) -> Option<PathBuf> {
            None
        }
        fn store(&self, _: &Path) -> Result<(), String> {
            Err("the registry exists only on Windows".to_string())
        }
    }
}

/// What the user typed for where the game is, as the content it names: the
/// folder given, plus each other wanted folder beside it.
pub fn located_from_answer(answer: &str, wanted: &[GameFolder], env: &dyn Environment) -> Result<Located, String> {
    let answer = answer.trim().trim_matches('"');
    if answer.is_empty() {
        return Err("no folder given".to_string());
    }
    let given = PathBuf::from(answer);
    if !env.is_dir(&given) {
        return Err(format!("{} is not a folder", given.display()));
    }
    let mut dirs = vec![given.clone()];
    if let Some(parent) = given.parent() {
        for game in wanted {
            let sibling = parent.join(game.folder);
            if sibling != given && env.is_dir(&sibling) && !dirs.contains(&sibling) {
                dirs.push(sibling);
            }
        }
    }
    Ok(Located { dirs, steam_home: None, found_by: FoundBy::Asked, cache: CacheOutcome::NotApplicable })
}

/// Searches the machine; failing that, asks on a terminal, or names
/// `--game-dir` in an input failure when nobody can be asked.
pub struct SystemLocator {
    pub env: Box<dyn Environment>,
    pub cache: Box<dyn HomeCache>,
    pub interactive: bool,
}

impl SystemLocator {
    /// The real machine, asking only when stdin and stderr are terminals.
    pub fn new() -> Self {
        Self {
            env: Box::new(SystemEnvironment),
            cache: system_cache(),
            interactive: std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
        }
    }
}

impl Default for SystemLocator {
    fn default() -> Self {
        Self::new()
    }
}

impl GameLocator for SystemLocator {
    fn locate(&self, games: &[GameFolder]) -> Result<Located, Failure> {
        let not_found = match locate(games, self.env.as_ref(), self.cache.as_ref()) {
            Ok(located) => return Ok(located),
            Err(not_found) => not_found,
        };
        if !self.interactive {
            return Err(Failure::input(not_found));
        }
        let example = games.first().map_or("tf", |g| g.folder);
        let mut stderr = std::io::stderr();
        let mut lines = std::io::stdin().lock().lines();
        for _ in 0..3 {
            let _ = write!(stderr, "{not_found}\nGame content folder (such as .../{example}), or empty to stop: ");
            let _ = stderr.flush();
            let Some(Ok(answer)) = lines.next() else {
                break;
            };
            if answer.trim().is_empty() {
                break;
            }
            match located_from_answer(&answer, games, self.env.as_ref()) {
                Ok(located) => return Ok(located),
                Err(reason) => {
                    let _ = writeln!(stderr, "{reason}");
                }
            }
        }
        Err(Failure::input(anyhow!("{not_found}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::locate::tests::Machine;

    const TF: GameFolder = GameFolder { app_id: 440, folder: "tf" };
    const HL2: GameFolder = GameFolder { app_id: 440, folder: "hl2" };

    #[test]
    fn the_file_cache_remembers_a_home_and_forgets_nothing_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache = FileCache::new(dir.path().join("nested/steam-home"));
        assert_eq!(cache.load(), None);
        cache.store(Path::new("/games/Steam")).expect("stored");
        assert_eq!(cache.load(), Some(PathBuf::from("/games/Steam")));
    }

    #[test]
    fn an_answer_naming_a_game_folder_brings_its_siblings() {
        let mut machine = Machine::new(Platform::Linux, "/home/p");
        machine.dir("/games/Team Fortress 2/tf").dir("/games/Team Fortress 2/hl2");
        let located = located_from_answer("  /games/Team Fortress 2/tf\n", &[TF, HL2], &machine).expect("valid");
        assert_eq!(
            located.dirs,
            vec![PathBuf::from("/games/Team Fortress 2/tf"), PathBuf::from("/games/Team Fortress 2/hl2")]
        );
        assert_eq!((located.found_by, located.cache), (FoundBy::Asked, CacheOutcome::NotApplicable));
    }

    #[test]
    fn an_answer_that_is_not_a_folder_is_refused() {
        let machine = Machine::new(Platform::Linux, "/home/p");
        assert!(located_from_answer("/nowhere", &[TF], &machine).is_err());
        assert!(located_from_answer("   ", &[TF], &machine).is_err());
    }

    #[test]
    fn the_platform_is_the_one_compiled_for() {
        let expected = if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        };
        assert_eq!(SystemEnvironment.platform(), expected);
    }
}
