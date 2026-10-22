//! A map's contents, and the world a player moves through once a game's
//! policy has said which of them block and which mark where players start,
//! and the prop models have said what shape each static prop is.

use std::collections::BTreeSet;

use crate::core::geom::{Mat3, Vec3};
use crate::core::solid::{ConvexSolid, Hull};

/// One map entity, with the keys navigation reads already typed.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub classname: String,
    pub origin: Vec3,
    /// The brush model of a `"model" "*N"` key.
    pub brush_model: Option<usize>,
    keys: Vec<(String, String)>,
}

impl Entity {
    pub fn new(classname: String, origin: Vec3, brush_model: Option<usize>, keys: Vec<(String, String)>) -> Self {
        Self { classname, origin, brush_model, keys }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WorldError {
    #[error("entity {index} ({classname}) uses brush model *{model}, but the map has {models}")]
    UnknownModel { index: usize, classname: String, model: usize, models: usize },
}

/// How a static prop collides, from its `solid` byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PropCollision {
    /// `SOLID_NONE`: players pass through.
    None,
    /// `SOLID_BBOX`: the model's hull box.
    BoundingBox,
    /// `SOLID_VPHYSICS`: the model's `.phy` collision hulls.
    VPhysics,
    /// A value the engine does not define for static props; not placed.
    Other(u8),
}

impl PropCollision {
    pub fn from_solid_type(solid: u8) -> Self {
        match solid {
            0 => PropCollision::None,
            2 => PropCollision::BoundingBox,
            6 => PropCollision::VPhysics,
            other => PropCollision::Other(other),
        }
    }
}

/// One `prop_static` as the map placed it.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticProp {
    /// The model's path in the game's content, such as `models/props_farm/wooden_barrel.mdl`.
    pub model: String,
    pub origin: Vec3,
    pub rotation: Mat3,
    pub collision: PropCollision,
}

/// Where prop models' collision shapes come from.
pub trait PropModels {
    /// The model's convex hulls for this kind of collision, in model space;
    /// `None` when the content holds no such model.
    fn hulls(&self, model: &str, collision: PropCollision) -> Option<&[Hull]>;
}

/// Everything solid a map holds, before any game says what it means:
/// model 0 is the world, the rest belong to brush entities.
#[derive(Clone, Debug, PartialEq)]
pub struct MapContents {
    entities: Vec<Entity>,
    models: Vec<Vec<ConvexSolid>>,
    displacements: Vec<ConvexSolid>,
    props: Vec<StaticProp>,
}

impl MapContents {
    /// Refuses an entity naming a brush model the map lacks, so assembly
    /// can take every model it is handed.
    pub fn new(
        entities: Vec<Entity>,
        models: Vec<Vec<ConvexSolid>>,
        displacements: Vec<ConvexSolid>,
        props: Vec<StaticProp>,
    ) -> Result<Self, WorldError> {
        for (index, entity) in entities.iter().enumerate() {
            if let Some(model) = entity.brush_model.filter(|m| *m >= models.len()) {
                return Err(WorldError::UnknownModel {
                    index,
                    classname: entity.classname.clone(),
                    model,
                    models: models.len(),
                });
            }
        }
        Ok(Self { entities, models, displacements, props })
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn props(&self) -> &[StaticProp] {
        &self.props
    }

    pub fn world_brushes(&self) -> &[ConvexSolid] {
        self.models.first().map(Vec::as_slice).unwrap_or(&[])
    }
}

/// What a game decides about a map's entities.
pub trait WorldPolicy {
    /// Whether a brush entity's model stands where the map placed it and blocks players.
    fn stays_solid(&self, entity: &Entity) -> bool;

    /// Whether the entity marks somewhere players start.
    fn is_spawn(&self, entity: &Entity) -> bool;
}

/// What became of a map's static props.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropSummary {
    /// Whether props were placed at all; without prop models, none are.
    pub placed: bool,
    pub total: usize,
    pub solid: usize,
    pub non_solid: usize,
    /// Models a solid prop names that the content lacks, each once, sorted.
    pub missing_models: Vec<String>,
}

/// The solids that block a player, and where players start.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub solids: Vec<ConvexSolid>,
    pub spawns: Vec<Vec3>,
    pub solid_brush_entities: usize,
    pub props: PropSummary,
}

impl World {
    /// Without `prop_models`, static props are left out and the summary
    /// says so.
    pub fn assemble(contents: &MapContents, policy: &dyn WorldPolicy, prop_models: Option<&dyn PropModels>) -> World {
        let mut solids = contents.world_brushes().to_vec();
        let mut solid_brush_entities = 0;
        for entity in &contents.entities {
            let Some(model) = entity.brush_model.filter(|m| *m > 0) else {
                continue;
            };
            if policy.stays_solid(entity) {
                solids.extend(contents.models[model].iter().map(|s| s.translated(entity.origin)));
                solid_brush_entities += 1;
            }
        }
        solids.extend(contents.displacements.iter().cloned());
        let props = match prop_models {
            Some(models) => place_props(&contents.props, models, &mut solids),
            None => PropSummary { placed: false, total: contents.props.len(), ..PropSummary::default() },
        };
        let spawns = contents.entities.iter().filter(|e| policy.is_spawn(e)).map(|e| e.origin).collect();
        World { solids, spawns, solid_brush_entities, props }
    }
}

fn place_props(props: &[StaticProp], models: &dyn PropModels, solids: &mut Vec<ConvexSolid>) -> PropSummary {
    let mut summary = PropSummary { placed: true, total: props.len(), ..PropSummary::default() };
    let mut missing = BTreeSet::new();
    for prop in props {
        match prop.collision {
            PropCollision::None | PropCollision::Other(_) => summary.non_solid += 1,
            PropCollision::BoundingBox | PropCollision::VPhysics => match models.hulls(&prop.model, prop.collision) {
                Some(hulls) => {
                    solids.extend(hulls.iter().map(|h| ConvexSolid::from_hull(h, prop.rotation, prop.origin)));
                    summary.solid += 1;
                }
                None => {
                    missing.insert(prop.model.clone());
                }
            },
        }
    }
    summary.missing_models = missing.into_iter().collect();
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geom::Aabb;

    fn cube(at: f32) -> ConvexSolid {
        ConvexSolid::axis_box(Aabb::from_corners(Vec3::splat(at), Vec3::splat(at + 8.0)))
    }

    fn entity(classname: &str, origin: Vec3, model: Option<usize>) -> Entity {
        Entity::new(classname.to_string(), origin, model, vec![])
    }

    /// Walls are solid, everything else is not; starts are spawns.
    struct WallsAndStarts;

    impl WorldPolicy for WallsAndStarts {
        fn stays_solid(&self, entity: &Entity) -> bool {
            entity.classname == "wall"
        }
        fn is_spawn(&self, entity: &Entity) -> bool {
            entity.classname == "start"
        }
    }

    #[test]
    fn a_model_the_map_lacks_is_refused() {
        let result = MapContents::new(vec![entity("wall", Vec3::ZERO, Some(3))], vec![vec![cube(0.0)]], vec![], vec![]);
        assert!(matches!(result, Err(WorldError::UnknownModel { index: 0, model: 3, models: 1, .. })));
    }

    #[test]
    fn the_policy_decides_which_brush_entities_block_and_where_they_stand() {
        let entities = vec![
            entity("wall", Vec3::new(100.0, 0.0, 0.0), Some(1)),
            entity("door", Vec3::ZERO, Some(2)),
            entity("start", Vec3::new(5.0, 6.0, 7.0), None),
        ];
        let contents = MapContents::new(
            entities,
            vec![vec![cube(0.0)], vec![cube(0.0)], vec![cube(50.0)]],
            vec![cube(-50.0)],
            vec![],
        )
        .expect("models exist");
        let world = World::assemble(&contents, &WallsAndStarts, None);
        assert_eq!(world.solids.len(), 3, "world, the wall, and the displacement; not the door");
        assert_eq!(world.solids[1].bounds().min(), Vec3::new(100.0, 0.0, 0.0));
        assert_eq!(world.solid_brush_entities, 1);
        assert_eq!(world.spawns, vec![Vec3::new(5.0, 6.0, 7.0)]);
    }

    /// A barrel whose collision is one 16-unit cube, at the model's origin.
    struct Barrel(Vec<Hull>);

    impl Barrel {
        fn new() -> Self {
            Barrel(vec![Hull::axis_box(Aabb::from_corners(Vec3::new(-8.0, -8.0, 0.0), Vec3::new(8.0, 8.0, 16.0)))])
        }
    }

    impl PropModels for Barrel {
        fn hulls(&self, model: &str, _: PropCollision) -> Option<&[Hull]> {
            (model == "models/barrel.mdl").then_some(self.0.as_slice())
        }
    }

    fn prop(model: &str, origin: Vec3, collision: PropCollision) -> StaticProp {
        StaticProp { model: model.to_string(), origin, rotation: Mat3::IDENTITY, collision }
    }

    fn props_world(props: Vec<StaticProp>, models: Option<&dyn PropModels>) -> World {
        let contents = MapContents::new(vec![], vec![vec![cube(0.0)]], vec![], props).expect("valid");
        World::assemble(&contents, &WallsAndStarts, models)
    }

    #[test]
    fn solid_types_map_to_collision_kinds() {
        assert_eq!(PropCollision::from_solid_type(0), PropCollision::None);
        assert_eq!(PropCollision::from_solid_type(2), PropCollision::BoundingBox);
        assert_eq!(PropCollision::from_solid_type(6), PropCollision::VPhysics);
        assert_eq!(PropCollision::from_solid_type(3), PropCollision::Other(3));
    }

    #[test]
    fn a_solid_prop_is_placed_where_the_map_put_it() {
        let world = props_world(
            vec![prop("models/barrel.mdl", Vec3::new(200.0, 0.0, 0.0), PropCollision::VPhysics)],
            Some(&Barrel::new()),
        );
        assert_eq!(world.solids.len(), 2);
        assert_eq!(world.solids[1].bounds().min(), Vec3::new(192.0, -8.0, 0.0));
        assert_eq!((world.props.placed, world.props.total, world.props.solid), (true, 1, 1));
    }

    #[test]
    fn props_players_pass_through_add_no_solids() {
        let props = vec![
            prop("models/barrel.mdl", Vec3::ZERO, PropCollision::None),
            prop("models/barrel.mdl", Vec3::ZERO, PropCollision::Other(9)),
        ];
        let world = props_world(props, Some(&Barrel::new()));
        assert_eq!(world.solids.len(), 1);
        assert_eq!((world.props.solid, world.props.non_solid), (0, 2));
    }

    #[test]
    fn a_model_the_content_lacks_is_reported_once() {
        let props = vec![
            prop("models/crate.mdl", Vec3::ZERO, PropCollision::VPhysics),
            prop("models/crate.mdl", Vec3::X, PropCollision::BoundingBox),
            prop("models/barrel.mdl", Vec3::ZERO, PropCollision::VPhysics),
        ];
        let world = props_world(props, Some(&Barrel::new()));
        assert_eq!(world.props.missing_models, vec!["models/crate.mdl".to_string()]);
        assert_eq!(world.props.solid, 1);
    }

    #[test]
    fn without_prop_models_no_props_are_placed() {
        let world = props_world(vec![prop("models/barrel.mdl", Vec3::ZERO, PropCollision::VPhysics)], None);
        assert_eq!(world.solids.len(), 1);
        assert_eq!((world.props.placed, world.props.total), (false, 1));
    }
}
