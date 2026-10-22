//! A map's contents, and the world a player moves through once a game's
//! policy has said which of them block and which mark where players start.

use crate::core::geom::Vec3;
use crate::core::solid::ConvexSolid;

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

/// Everything solid a map holds, before any game says what it means:
/// model 0 is the world, the rest belong to brush entities.
#[derive(Clone, Debug, PartialEq)]
pub struct MapContents {
    entities: Vec<Entity>,
    models: Vec<Vec<ConvexSolid>>,
    displacements: Vec<ConvexSolid>,
}

impl MapContents {
    /// Refuses an entity naming a brush model the map lacks, so assembly
    /// can take every model it is handed.
    pub fn new(
        entities: Vec<Entity>,
        models: Vec<Vec<ConvexSolid>>,
        displacements: Vec<ConvexSolid>,
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
        Ok(Self { entities, models, displacements })
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
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

/// The solids that block a player, and where players start.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub solids: Vec<ConvexSolid>,
    pub spawns: Vec<Vec3>,
    pub solid_brush_entities: usize,
}

impl World {
    pub fn assemble(contents: &MapContents, policy: &dyn WorldPolicy) -> World {
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
        let spawns = contents.entities.iter().filter(|e| policy.is_spawn(e)).map(|e| e.origin).collect();
        World { solids, spawns, solid_brush_entities }
    }
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
        let result = MapContents::new(vec![entity("wall", Vec3::ZERO, Some(3))], vec![vec![cube(0.0)]], vec![]);
        assert!(matches!(result, Err(WorldError::UnknownModel { index: 0, model: 3, models: 1, .. })));
    }

    #[test]
    fn the_policy_decides_which_brush_entities_block_and_where_they_stand() {
        let entities = vec![
            entity("wall", Vec3::new(100.0, 0.0, 0.0), Some(1)),
            entity("door", Vec3::ZERO, Some(2)),
            entity("start", Vec3::new(5.0, 6.0, 7.0), None),
        ];
        let contents =
            MapContents::new(entities, vec![vec![cube(0.0)], vec![cube(0.0)], vec![cube(50.0)]], vec![cube(-50.0)])
                .expect("models exist");
        let world = World::assemble(&contents, &WallsAndStarts);
        assert_eq!(world.solids.len(), 3, "world, the wall, and the displacement; not the door");
        assert_eq!(world.solids[1].bounds().min(), Vec3::new(100.0, 0.0, 0.0));
        assert_eq!(world.solid_brush_entities, 1);
        assert_eq!(world.spawns, vec![Vec3::new(5.0, 6.0, 7.0)]);
    }
}
