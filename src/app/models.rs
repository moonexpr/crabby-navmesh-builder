//! Prop models' collision shapes, loaded once each from a game's content.

use std::collections::{BTreeSet, HashMap};

use rayon::prelude::*;
use serde::Serialize;

use crate::core::solid::Hull;
use crate::core::world::{PropCollision, PropModels, StaticProp};
use crate::format::model::{mdl_hull_box, phy_hulls};

use super::content::Content;

/// A model the content holds but that could not be read.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BrokenModel {
    pub path: String,
    pub reason: String,
}

/// The collision hulls of every model the props need that the content has.
#[derive(Debug, Default)]
pub struct ModelLibrary {
    hulls: HashMap<ModelKey, Vec<Hull>>,
    broken: Vec<BrokenModel>,
}

impl ModelLibrary {
    /// Loads, in parallel, each distinct model and collision kind the solid
    /// props name: a `.phy` for `VPhysics`, the `.mdl` header's hull box for
    /// `BoundingBox`.
    pub fn load(content: &dyn Content, props: &[StaticProp]) -> Self {
        let wanted: BTreeSet<(String, PropCollision)> = props
            .iter()
            .filter(|p| matches!(p.collision, PropCollision::VPhysics | PropCollision::BoundingBox))
            .map(|p| (p.model.clone(), p.collision))
            .collect();
        let loaded: Vec<(ModelKey, Load)> = wanted
            .into_par_iter()
            .map(|(model, collision)| {
                let load = load_model(content, &model, collision);
                ((model, collision), load)
            })
            .collect();
        let mut library = ModelLibrary::default();
        for (key, load) in loaded {
            match load {
                Load::Found(hulls) => {
                    library.hulls.insert(key, hulls);
                }
                Load::Absent => {}
                Load::Broken(broken) => library.broken.push(broken),
            }
        }
        library.broken.sort_by(|a, b| a.path.cmp(&b.path));
        library
    }

    pub fn len(&self) -> usize {
        self.hulls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hulls.is_empty()
    }

    /// Models found but unreadable, sorted by path.
    pub fn broken(&self) -> &[BrokenModel] {
        &self.broken
    }
}

impl PropModels for ModelLibrary {
    fn hulls(&self, model: &str, collision: PropCollision) -> Option<&[Hull]> {
        self.hulls.get(&(model.to_string(), collision)).map(Vec::as_slice)
    }
}

type ModelKey = (String, PropCollision);

/// What looking for one model's collision found.
enum Load {
    Found(Vec<Hull>),
    Absent,
    Broken(BrokenModel),
}

fn load_model(content: &dyn Content, model: &str, collision: PropCollision) -> Load {
    let file = collision_file(model, collision);
    let bytes = match content.read(&file) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Load::Absent,
        Err(e) => return Load::Broken(BrokenModel { path: file, reason: e.to_string() }),
    };
    let hulls = match collision {
        PropCollision::VPhysics => phy_hulls(&bytes),
        _ => mdl_hull_box(&bytes).map(|hull| vec![hull]),
    };
    match hulls {
        Ok(hulls) => Load::Found(hulls),
        Err(e) => Load::Broken(BrokenModel { path: file, reason: e.to_string() }),
    }
}

/// The file a prop's collision lives in: `x.phy` beside `x.mdl`, or the model itself.
fn collision_file(model: &str, collision: PropCollision) -> String {
    match collision {
        PropCollision::VPhysics => format!("{}.phy", model.strip_suffix(".mdl").unwrap_or(model)),
        _ => model.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geom::{Mat3, Vec3};
    use crate::format::model::tests::mdl_with_hull;

    #[derive(Debug)]
    struct Files(Vec<(&'static str, Vec<u8>)>);

    impl Content for Files {
        fn read(&self, path: &str) -> Result<Option<Vec<u8>>, super::super::content::ContentError> {
            Ok(self.0.iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone()))
        }
    }

    fn prop(model: &str, collision: PropCollision) -> StaticProp {
        StaticProp { model: model.to_string(), origin: Vec3::ZERO, rotation: Mat3::IDENTITY, collision }
    }

    #[test]
    fn collision_lives_in_the_phy_beside_the_mdl_or_in_the_mdl() {
        assert_eq!(collision_file("models/a/b.mdl", PropCollision::VPhysics), "models/a/b.phy");
        assert_eq!(collision_file("models/a/b.mdl", PropCollision::BoundingBox), "models/a/b.mdl");
    }

    #[test]
    fn a_bounding_box_prop_takes_its_mdl_hull_box() {
        let content = Files(vec![("models/crate.mdl", mdl_with_hull(Vec3::splat(-10.0), Vec3::splat(10.0)))]);
        let library = ModelLibrary::load(&content, &[prop("models/crate.mdl", PropCollision::BoundingBox)]);
        let hulls = library.hulls("models/crate.mdl", PropCollision::BoundingBox).expect("loaded");
        assert_eq!(hulls.len(), 1);
        assert!(library.broken().is_empty());
    }

    #[test]
    fn missing_and_broken_models_are_told_apart() {
        let content = Files(vec![("models/rock.phy", b"not a phy".to_vec())]);
        let props =
            [prop("models/rock.mdl", PropCollision::VPhysics), prop("models/gone.mdl", PropCollision::VPhysics)];
        let library = ModelLibrary::load(&content, &props);
        assert!(library.is_empty());
        assert_eq!(library.broken().len(), 1);
        assert_eq!(library.broken()[0].path, "models/rock.phy");
    }

    #[test]
    fn props_players_pass_through_load_nothing() {
        let content = Files(vec![]);
        let library = ModelLibrary::load(&content, &[prop("models/rock.mdl", PropCollision::None)]);
        assert!(library.is_empty() && library.broken().is_empty());
    }
}
