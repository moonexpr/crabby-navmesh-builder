mod support;

use std::collections::{HashMap, HashSet, VecDeque};

use crabby_navmesh_builder::core::generate::{GenerateError, Params, generate};
use crabby_navmesh_builder::core::geom::Vec3;
use crabby_navmesh_builder::core::mesh::{Attributes, Direction, NavArea};

use support::{area_at, connects, solid, world};

fn floor() -> crabby_navmesh_builder::core::solid::ConvexSolid {
    solid((0.0, 0.0, -16.0), (500.0, 500.0, 0.0))
}

fn areas(solids: Vec<crabby_navmesh_builder::core::solid::ConvexSolid>, seeds: &[Vec3]) -> Vec<NavArea> {
    generate(&world(solids), seeds, &Params::default()).expect("generates").areas
}

/// Every area id reachable from the first area along connections.
fn reachable_ids(areas: &[NavArea]) -> HashSet<u32> {
    reachable_from(areas, &areas[0])
}

fn path_exists(areas: &[NavArea], from: &NavArea, to: &NavArea) -> bool {
    reachable_from(areas, from).contains(&to.id().get())
}

fn reachable_from(areas: &[NavArea], start: &NavArea) -> HashSet<u32> {
    let by_id: HashMap<u32, &NavArea> = areas.iter().map(|a| (a.id().get(), a)).collect();
    let mut seen = HashSet::from([start.id().get()]);
    let mut queue = VecDeque::from([start.id().get()]);
    while let Some(id) = queue.pop_front() {
        for d in Direction::ALL {
            for next in by_id[&id].connections(d) {
                if seen.insert(next.get()) {
                    queue.push_back(next.get());
                }
            }
        }
    }
    seen
}

#[test]
fn an_open_floor_becomes_one_area_covering_it() {
    let result = areas(vec![floor()], &[Vec3::new(250.0, 250.0, 1.0)]);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].north_west(), Vec3::new(0.0, 0.0, 0.0));
    assert_eq!(result[0].south_east(), Vec3::new(500.0, 500.0, 0.0));
    assert_eq!(result[0].id().get(), 1);
}

#[test]
fn rooms_joined_by_a_doorway_are_connected_both_ways() {
    let wall_south = solid((240.0, 0.0, 0.0), (260.0, 200.0, 128.0));
    let wall_north = solid((240.0, 300.0, 0.0), (260.0, 500.0, 128.0));
    let result = areas(vec![floor(), wall_south, wall_north], &[Vec3::new(100.0, 100.0, 1.0)]);
    let west = area_at(&result, 100.0, 100.0, 0.0).expect("west room");
    let east = area_at(&result, 400.0, 100.0, 0.0).expect("east room is reachable");
    assert_ne!(west.id(), east.id());
    assert_eq!(reachable_ids(&result).len(), result.len(), "one connected mesh");
    for a in &result {
        for d in Direction::ALL {
            for to in a.connections(d) {
                let back = result.iter().find(|b| b.id() == *to).expect("connections name real areas");
                assert!(back.connections(d.opposite()).contains(&a.id()), "flat walks are two-way");
            }
        }
    }
    assert!(area_at(&result, 250.0, 100.0, 0.0).is_none(), "no area inside the wall");
}

#[test]
fn a_sealed_room_is_left_out() {
    let wall = solid((240.0, 0.0, 0.0), (260.0, 500.0, 128.0));
    let result = areas(vec![floor(), wall], &[Vec3::new(100.0, 100.0, 1.0)]);
    assert!(area_at(&result, 400.0, 100.0, 0.0).is_none());
    assert!(area_at(&result, 100.0, 100.0, 0.0).is_some());
}

#[test]
fn a_low_ceiling_makes_crouch_areas() {
    let ceiling = solid((300.0, 0.0, 60.0), (500.0, 500.0, 76.0));
    let result = areas(vec![floor(), ceiling], &[Vec3::new(100.0, 100.0, 1.0)]);
    let under = area_at(&result, 450.0, 250.0, 0.0).expect("crouch-height space is walkable");
    let open = area_at(&result, 100.0, 250.0, 0.0).expect("open floor");
    assert!(under.attributes().contains(Attributes::CROUCH));
    assert!(!open.attributes().contains(Attributes::CROUCH));
    assert!(connects(open, under) && connects(under, open));
}

#[test]
fn a_ceiling_too_low_to_crouch_under_is_left_out() {
    let ceiling = solid((300.0, 0.0, 40.0), (500.0, 500.0, 120.0));
    let result = areas(vec![floor(), ceiling], &[Vec3::new(100.0, 100.0, 1.0)]);
    assert!(area_at(&result, 450.0, 250.0, 0.0).is_none());
}

#[test]
fn a_step_splits_areas_that_still_walk_both_ways() {
    let raised = solid((250.0, 0.0, 0.0), (500.0, 500.0, 12.0));
    let result = areas(vec![floor(), raised], &[Vec3::new(100.0, 100.0, 1.0)]);
    let low = area_at(&result, 100.0, 250.0, 0.0).expect("low side");
    let high = area_at(&result, 400.0, 250.0, 12.0).expect("high side");
    assert_eq!(high.north_west().z, 12.0);
    assert!(path_exists(&result, low, high) && path_exists(&result, high, low));
}

#[test]
fn a_ledge_connects_down_but_not_back_up() {
    let platform = solid((0.0, 0.0, 0.0), (250.0, 500.0, 100.0));
    let result = areas(vec![floor(), platform], &[Vec3::new(100.0, 250.0, 101.0)]);
    let top = area_at(&result, 100.0, 250.0, 100.0).expect("platform");
    let bottom = area_at(&result, 450.0, 250.0, 0.0).expect("reachable by dropping");
    assert_eq!(top.north_west().z, 100.0);
    assert_eq!(bottom.north_west().z, 0.0);
    let edge_below = result
        .iter()
        .filter(|a| a.north_west().z == 0.0)
        .find(|a| connects(top, a))
        .expect("the platform drops onto the floor");
    assert!(!connects(edge_below, top), "100 units is too high to jump");
}

#[test]
fn starting_below_a_ledge_cannot_reach_its_top() {
    let platform = solid((0.0, 0.0, 0.0), (250.0, 500.0, 100.0));
    let result = areas(vec![floor(), platform], &[Vec3::new(450.0, 250.0, 1.0)]);
    assert!(result.iter().all(|a| a.north_west().z == 0.0));
}

#[test]
fn a_crate_low_enough_to_jump_onto_connects_up() {
    let crate_box = solid((200.0, 200.0, 0.0), (300.0, 300.0, 40.0));
    let result = areas(vec![floor(), crate_box], &[Vec3::new(50.0, 50.0, 1.0)]);
    let top = area_at(&result, 250.0, 250.0, 40.0).expect("crate top is reachable by jumping");
    assert_eq!(top.north_west().z, 40.0);
}

#[test]
fn every_area_has_positive_extent_and_unique_id() {
    let crate_box = solid((200.0, 200.0, 0.0), (300.0, 300.0, 40.0));
    let ceiling = solid((300.0, 0.0, 60.0), (500.0, 200.0, 76.0));
    let result = areas(vec![floor(), crate_box, ceiling], &[Vec3::new(50.0, 50.0, 1.0)]);
    let ids: HashSet<u32> = result.iter().map(|a| a.id().get()).collect();
    assert_eq!(ids.len(), result.len());
    assert_eq!(ids, (1..=result.len() as u32).collect());
}

#[test]
fn the_result_does_not_depend_on_the_thread_count() {
    let solids = || {
        vec![
            floor(),
            solid((200.0, 200.0, 0.0), (300.0, 300.0, 40.0)),
            solid((300.0, 0.0, 60.0), (500.0, 200.0, 76.0)),
            solid((0.0, 400.0, 0.0), (500.0, 410.0, 128.0)),
        ]
    };
    let seeds = [Vec3::new(50.0, 50.0, 1.0)];
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().expect("pool");
        pool.install(|| generate(&world(solids()), &seeds, &Params::default()).expect("generates").areas)
    };
    assert_eq!(run(1), run(8));
}

#[test]
fn no_seeds_is_an_error() {
    let result = generate(&world(vec![floor()]), &[], &Params::default());
    assert_eq!(result.err(), Some(GenerateError::NoSeeds));
}

#[test]
fn seeds_floating_in_the_void_are_an_error() {
    let result = generate(&world(vec![floor()]), &[Vec3::new(5000.0, 5000.0, 0.0)], &Params::default());
    assert_eq!(result.err(), Some(GenerateError::NoSeedOnMesh(1)));
}

#[test]
fn stats_report_seeds_that_missed() {
    let seeds = [Vec3::new(100.0, 100.0, 1.0), Vec3::new(5000.0, 5000.0, 0.0)];
    let generated = generate(&world(vec![floor()]), &seeds, &Params::default()).expect("one seed is enough");
    assert_eq!(generated.stats.seeds_used, 1);
    assert_eq!(generated.stats.seeds_off_mesh, vec![Vec3::new(5000.0, 5000.0, 0.0)]);
    assert_eq!(generated.stats.areas, generated.areas.len());
}
