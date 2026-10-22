mod support;

use crabby_navmesh_builder::core::collision::{Collision, SolidGrid};
use crabby_navmesh_builder::core::geom::{Plane, Vec3};
use crabby_navmesh_builder::core::solid::{AxialSide, SolidError};
use crabby_navmesh_builder::core::world::{Contents, World, WorldError};
use crabby_navmesh_builder::format::bsp::{BspError, parse};
use crabby_navmesh_builder::spec::Build;

use crabby_navmesh_builder::core::world::PropCollision;
use crabby_navmesh_builder::format::bsp::pakfile_files;
use support::bsp_writer::{
    BspWriter, CONTENTS_PLAYERCLIP, CONTENTS_SOLID, CONTENTS_WATER, SOLID_BBOX, SOLID_NONE, SOLID_VPHYSICS, box_planes,
};

/// The map's world as TF2 sees it.
fn tf_world(bytes: &[u8]) -> World {
    let map = parse(bytes).expect("parses");
    World::assemble(&map.contents, Build::Tf.spec().expect("supported").policy(), None, Contents::PLAYER_SOLID)
}

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

#[test]
fn world_brushes_become_solids_with_their_bounds() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.box_brush(0, v(0.0, 0.0, 0.0), v(16.0, 512.0, 128.0), CONTENTS_SOLID);
    let bytes = map.build(20);
    let parsed = parse(&bytes).expect("parses");
    assert_eq!(parsed.version, 20);
    assert_eq!(parsed.stats.brushes, 2);
    let world = tf_world(&bytes);
    assert_eq!(world.solids.len(), 2);
    assert_eq!(world.solids[0].bounds().min(), v(0.0, 0.0, -16.0));
    assert_eq!(world.solids[1].bounds().max(), v(16.0, 512.0, 128.0));
}

#[test]
fn only_brushes_that_block_players_are_kept() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, 0.0), v(64.0, 64.0, 64.0), CONTENTS_WATER);
    map.box_brush(0, v(100.0, 0.0, 0.0), v(164.0, 64.0, 64.0), CONTENTS_PLAYERCLIP);
    let world = tf_world(&map.build(20));
    assert_eq!(world.solids.len(), 1);
    assert_eq!(world.solids[0].bounds().min(), v(100.0, 0.0, 0.0));
}

#[test]
fn solid_brush_entities_move_to_their_origin_and_doors_are_ignored() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    let wall = map.add_model();
    map.box_brush(wall, v(-8.0, -8.0, 0.0), v(8.0, 8.0, 64.0), CONTENTS_SOLID);
    let door = map.add_model();
    map.box_brush(door, v(-8.0, -8.0, 0.0), v(8.0, 8.0, 64.0), CONTENTS_SOLID);
    map.entity(&[("classname", "func_brush"), ("model", "*1"), ("origin", "100 200 0")]);
    map.entity(&[("classname", "func_door"), ("model", "*2"), ("origin", "300 300 0")]);
    let bytes = map.build(20);
    assert_eq!(parse(&bytes).expect("parses").stats.brush_models, 2);
    let world = tf_world(&bytes);
    assert_eq!(world.solids.len(), 2);
    assert_eq!(world.solids[1].bounds().min(), v(92.0, 192.0, 0.0));
    assert_eq!(world.solid_brush_entities, 1);
}

#[test]
fn a_never_solid_func_brush_is_ignored() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    let model = map.add_model();
    map.box_brush(model, v(0.0, 0.0, 0.0), v(8.0, 8.0, 64.0), CONTENTS_SOLID);
    map.entity(&[("classname", "func_brush"), ("model", "*1"), ("Solidity", "1")]);
    assert_eq!(tf_world(&map.build(20)).solids.len(), 1);
}

#[test]
fn brush_entities_the_map_can_toggle_are_ignored() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    for _ in 0..3 {
        let model = map.add_model();
        map.box_brush(model, v(0.0, 0.0, 0.0), v(8.0, 8.0, 64.0), CONTENTS_SOLID);
    }
    map.entity(&[("classname", "func_brush"), ("model", "*1"), ("targetname", "gate_5a")]);
    map.entity(&[("classname", "func_brush"), ("model", "*2"), ("StartDisabled", "1")]);
    map.entity(&[("classname", "func_wall_toggle"), ("model", "*3"), ("targetname", "fence")]);
    let world = tf_world(&map.build(20));
    assert_eq!(world.solids.len(), 1, "gates opened by map logic must not wall off the mesh");
    assert_eq!(world.solid_brush_entities, 0);
}

#[test]
fn entities_come_through_typed() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.entity(&[("classname", "info_player_teamspawn"), ("origin", "10 20 30")]);
    let parsed = parse(&map.build(20)).expect("parses");
    let spawn = parsed.contents.entities().iter().find(|e| e.classname == "info_player_teamspawn").expect("present");
    assert_eq!(spawn.origin, v(10.0, 20.0, 30.0));
}

#[test]
fn a_displacement_becomes_walkable_prisms_at_its_displaced_height() {
    let mut map = BspWriter::new();
    let corners = [v(0.0, 0.0, 0.0), v(0.0, 256.0, 0.0), v(256.0, 256.0, 0.0), v(256.0, 0.0, 0.0)];
    let mut offsets = vec![Vec3::ZERO; 25];
    for o in offsets.iter_mut() {
        *o = v(0.0, 0.0, 32.0);
    }
    map.displacement(corners, 2, offsets);
    let bytes = map.build(20);
    let parsed = parse(&bytes).expect("parses");
    assert_eq!(parsed.stats.displacements, 1);
    assert_eq!(parsed.stats.displacement_triangles, 32);
    let world = SolidGrid::new(tf_world(&bytes).solids).expect("not empty");
    let spans = world.column(100.0, 100.0);
    assert_eq!(spans.len(), 1);
    assert!((spans[0].top - 32.0).abs() < 1e-3, "top was {}", spans[0].top);
    assert!(spans[0].top_normal.z > 0.99);
}

#[test]
fn a_brush_without_a_bevel_plane_is_refused() {
    let mut map = BspWriter::new();
    let mut planes = box_planes(v(0.0, 0.0, 0.0), v(64.0, 64.0, 64.0));
    planes.retain(|p| p.normal != Vec3::Z);
    planes.push(Plane { normal: v(0.0, 0.6, 0.8), dist: 60.0 });
    map.brush(0, &planes, CONTENTS_SOLID);
    let result = parse(&map.build(20));
    assert_eq!(result.err(), Some(BspError::Brush { brush: 0, source: SolidError::Unbounded(AxialSide::MaxZ) }));
}

#[test]
fn version_19_maps_with_wide_leaves_parse() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    assert_eq!(tf_world(&map.build(19)).solids.len(), 1);
}

#[test]
fn a_brush_index_past_the_lump_is_refused() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    let mut bytes = map.build(20);
    // Point the world leaf's first brush at brush 7, which does not exist.
    let leaf_entry = 8 + 17 * 16;
    let leaf_brushes = u32::from_le_bytes(bytes[leaf_entry..leaf_entry + 4].try_into().expect("4 bytes")) as usize;
    bytes[leaf_brushes..leaf_brushes + 2].copy_from_slice(&7u16.to_le_bytes());
    assert!(matches!(parse(&bytes), Err(BspError::BadIndex { what: "brush", index: 7 })));
}

#[test]
fn an_entity_naming_a_missing_brush_model_is_refused_at_parse() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.entity(&[("classname", "func_brush"), ("model", "*4")]);
    assert!(matches!(parse(&map.build(20)), Err(BspError::World(WorldError::UnknownModel { model: 4, .. }))));
}

#[test]
fn static_props_come_through_with_normalized_models_and_their_rotation() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.static_prop("Models\\Props_Farm/Barrel.mdl", v(100.0, 200.0, 0.0), [0.0, 90.0, 0.0], SOLID_VPHYSICS);
    map.static_prop("models/props_farm/crate.mdl", v(300.0, 300.0, 0.0), [0.0, 0.0, 0.0], SOLID_BBOX);
    map.static_prop("models/props_farm/crate.mdl", v(50.0, 50.0, 0.0), [0.0, 0.0, 0.0], SOLID_NONE);
    let parsed = parse(&map.build(20)).expect("parses");
    let props = parsed.contents.props();
    assert_eq!(parsed.stats.static_props, 3);
    let barrel = props.iter().find(|p| p.model == "models/props_farm/barrel.mdl").expect("normalized path");
    assert_eq!(barrel.origin, v(100.0, 200.0, 0.0));
    assert_eq!(barrel.collision, PropCollision::VPhysics);
    assert!((barrel.rotation * Vec3::X - Vec3::Y).length() < 1e-5, "yaw 90 turns forward to +y");
    let collisions: Vec<PropCollision> = props.iter().map(|p| p.collision).collect();
    assert!(collisions.contains(&PropCollision::BoundingBox) && collisions.contains(&PropCollision::None));
}

#[test]
fn a_map_without_static_props_has_none() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    assert!(parse(&map.build(20)).expect("parses").contents.props().is_empty());
}

#[test]
fn pakfile_files_are_read_by_normalized_path() {
    let mut map = BspWriter::new();
    map.box_brush(0, v(0.0, 0.0, -16.0), v(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.pack("Models/Custom/Rock.PHY", b"phy bytes");
    map.pack("materials/custom/rock.vmt", b"vmt bytes");
    let files = pakfile_files(&map.build(20), |path| path.ends_with(".phy")).expect("readable");
    assert_eq!(files, vec![("models/custom/rock.phy".to_string(), b"phy bytes".to_vec())]);
}
