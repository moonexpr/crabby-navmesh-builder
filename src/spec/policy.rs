//! The Source engine's common rules for a map's entities, shared by the
//! games built on it.

use crate::core::world::{Entity, WorldPolicy};

/// Classnames whose origins mark where players start.
const SPAWN_CLASSES: [&str; 7] = [
    "info_player_teamspawn",
    "info_player_start",
    "info_player_deathmatch",
    "info_player_combine",
    "info_player_rebel",
    "info_player_terrorist",
    "info_player_counterterrorist",
];

/// Brush entities that can stand where the map placed them and block
/// players. Doors, platforms, and team-filtered walls move or open, so the
/// mesh passes through them as `nav_generate` does; so does any of these
/// the map's logic can reach by name, or that starts disabled, since a gate
/// opened mid-round must not split the mesh.
const STATIC_BRUSH_CLASSES: [&str; 3] = ["func_brush", "func_wall", "func_wall_toggle"];

/// `func_brush`'s "Solidity" value for never solid.
const NEVER_SOLID: &str = "1";

#[derive(Clone, Copy, Debug, Default)]
pub struct SourcePolicy;

impl WorldPolicy for SourcePolicy {
    fn stays_solid(&self, entity: &Entity) -> bool {
        STATIC_BRUSH_CLASSES.contains(&entity.classname.as_str())
            && entity.get("Solidity") != Some(NEVER_SOLID)
            && entity.get("StartDisabled") != Some("1")
            && entity.get("targetname").is_none_or(str::is_empty)
    }

    fn is_spawn(&self, entity: &Entity) -> bool {
        SPAWN_CLASSES.contains(&entity.classname.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::geom::Vec3;

    fn entity(classname: &str, keys: &[(&str, &str)]) -> Entity {
        let keys = keys.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Entity::new(classname.to_string(), Vec3::ZERO, Some(1), keys)
    }

    #[test]
    fn an_unnamed_func_brush_stays_solid() {
        assert!(SourcePolicy.stays_solid(&entity("func_brush", &[])));
        assert!(SourcePolicy.stays_solid(&entity("func_wall", &[("targetname", "")])));
    }

    #[test]
    fn doors_and_never_solid_brushes_do_not_block() {
        assert!(!SourcePolicy.stays_solid(&entity("func_door", &[])));
        assert!(!SourcePolicy.stays_solid(&entity("func_brush", &[("Solidity", "1")])));
    }

    #[test]
    fn brushes_the_map_can_toggle_do_not_block() {
        assert!(!SourcePolicy.stays_solid(&entity("func_brush", &[("targetname", "gate_5a")])));
        assert!(!SourcePolicy.stays_solid(&entity("func_brush", &[("StartDisabled", "1")])));
        assert!(!SourcePolicy.stays_solid(&entity("func_wall_toggle", &[("targetname", "fence")])));
    }

    #[test]
    fn player_starts_of_every_source_game_are_spawns() {
        for class in ["info_player_teamspawn", "info_player_start", "info_player_terrorist"] {
            assert!(SourcePolicy.is_spawn(&entity(class, &[])), "{class}");
        }
        assert!(!SourcePolicy.is_spawn(&entity("info_target", &[])));
    }
}
