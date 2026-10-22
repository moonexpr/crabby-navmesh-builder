//! What each game expects: the nav dialect its engine loads, and its policy
//! for a map's entities. [`Build`] names a game; [`Build::spec`] is the one
//! place a name becomes an implementation, or is refused as unsupported.

mod gmod;
mod policy;
mod tf;

use std::fmt;
use std::str::FromStr;

use crate::core::generate::Params;
use crate::core::world::WorldPolicy;
use crate::format::nav::NavDialect;

pub use gmod::Gmod;
pub use policy::SourcePolicy;
pub use tf::Tf;

/// A game whose navigation meshes this tool could target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Build {
    Tf,
    Gmod,
    Hl2,
    Css,
    Csgo,
}

impl Build {
    pub const ALL: [Build; 5] = [Build::Tf, Build::Gmod, Build::Hl2, Build::Css, Build::Csgo];

    pub fn name(self) -> &'static str {
        match self {
            Build::Tf => "tf",
            Build::Gmod => "gmod",
            Build::Hl2 => "hl2",
            Build::Css => "css",
            Build::Csgo => "csgo",
        }
    }

    /// The implementation for this game, if there is one yet.
    pub fn spec(self) -> Result<&'static dyn GameSpec, SpecError> {
        match self {
            Build::Tf => Ok(&Tf),
            Build::Gmod => Ok(&Gmod),
            Build::Hl2 | Build::Css | Build::Csgo => Err(SpecError::Unsupported(self)),
        }
    }
}

impl fmt::Display for Build {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Build {
    type Err = SpecError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Build::ALL.into_iter().find(|b| b.name() == text).ok_or_else(|| SpecError::Unknown(text.to_string()))
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SpecError {
    #[error("unknown build {0:?}; builds are tf, gmod, hl2, css, csgo")]
    Unknown(String),
    #[error("{0} navigation meshes are not supported yet; supported builds are tf and gmod")]
    Unsupported(Build),
}

/// Everything particular to one game.
pub trait GameSpec: fmt::Debug + Send + Sync {
    fn build(&self) -> Build;

    /// The nav layout the game's engine loads.
    fn dialect(&self) -> &dyn NavDialect;

    /// Which entities block players and which mark where they start.
    fn policy(&self) -> &dyn WorldPolicy;

    /// The agent the mesh is generated for.
    fn params(&self) -> Params {
        Params::default()
    }
}

/// The supported game whose dialect writes this sub-version.
pub fn for_sub_version(sub_version: u32) -> Option<&'static dyn GameSpec> {
    Build::ALL.into_iter().filter_map(|b| b.spec().ok()).find(|s| s.dialect().sub_version() == sub_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_build_name_parses_back_to_itself() {
        for build in Build::ALL {
            assert_eq!(build.to_string().parse::<Build>(), Ok(build));
        }
    }

    #[test]
    fn an_unknown_name_is_refused() {
        assert_eq!("quake".parse::<Build>(), Err(SpecError::Unknown("quake".to_string())));
    }

    #[test]
    fn tf_and_gmod_have_specs_and_the_rest_are_refused() {
        assert_eq!(Build::Tf.spec().map(|s| s.build()), Ok(Build::Tf));
        assert_eq!(Build::Gmod.spec().map(|s| s.build()), Ok(Build::Gmod));
        for build in [Build::Hl2, Build::Css, Build::Csgo] {
            assert_eq!(build.spec().map(|s| s.build()), Err(SpecError::Unsupported(build)));
        }
    }

    #[test]
    fn a_sub_version_names_its_game() {
        assert_eq!(for_sub_version(2).map(|s| s.build()), Some(Build::Tf));
        assert_eq!(for_sub_version(0).map(|s| s.build()), Some(Build::Gmod));
        assert_eq!(for_sub_version(1).map(|s| s.build()), None, "CS:S is not supported yet");
    }
}
