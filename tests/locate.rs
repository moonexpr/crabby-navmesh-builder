mod support;

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use crabby_navmesh_builder::app::Failure;
use crabby_navmesh_builder::app::FailureKind;
use crabby_navmesh_builder::app::generate::{GenerateRequest, generate_nav};
use crabby_navmesh_builder::app::locate::{CacheOutcome, FoundBy, GameLocator, Located};
use crabby_navmesh_builder::core::geom::Vec3;
use crabby_navmesh_builder::spec::{Build, GameFolder};

use support::bsp_writer::{BspWriter, CONTENTS_SOLID, SOLID_BBOX};
use support::mdl_with_hull;

const CRATE: &str = "models/test/crate.mdl";

/// A map saved outside any game's `maps/`, so nothing says where its game is.
fn loose_map(dir: &Path, solid_prop: bool) -> PathBuf {
    let mut map = BspWriter::new();
    map.box_brush(0, Vec3::new(0.0, 0.0, -16.0), Vec3::new(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.entity(&[("classname", "info_player_teamspawn"), ("origin", "100 100 8")]);
    if solid_prop {
        map.static_prop(CRATE, Vec3::new(300.0, 300.0, 0.0), [0.0, 0.0, 0.0], SOLID_BBOX);
    }
    let path = dir.join("loose.bsp");
    std::fs::write(&path, map.build(20)).expect("written");
    path
}

fn request(bsp: PathBuf) -> GenerateRequest {
    GenerateRequest {
        bsp,
        output: None,
        build: Build::Tf,
        seeds: vec![],
        cell_size: None,
        merge_tolerance: None,
        props: true,
        game_dirs: vec![],
    }
}

/// Finds the game in a folder of the test's making, and records what it was asked for.
struct Finds(PathBuf, std::cell::RefCell<Vec<&'static str>>);

impl GameLocator for Finds {
    fn locate(&self, games: &[GameFolder]) -> Result<Located, Failure> {
        self.1.borrow_mut().extend(games.iter().map(|g| g.folder));
        Ok(Located {
            dirs: vec![self.0.clone()],
            steam_home: None,
            found_by: FoundBy::DefaultPath,
            cache: CacheOutcome::Stored,
        })
    }
}

struct FindsNothing;

impl GameLocator for FindsNothing {
    fn locate(&self, _: &[GameFolder]) -> Result<Located, Failure> {
        Err(Failure::input(anyhow!("found no installed tf content; pass --game-dir")))
    }
}

struct MustNotBeAsked;

impl GameLocator for MustNotBeAsked {
    fn locate(&self, _: &[GameFolder]) -> Result<Located, Failure> {
        panic!("the machine was searched though nothing needed it");
    }
}

#[test]
fn a_game_found_on_the_machine_supplies_the_props_models() {
    let dir = tempfile::tempdir().expect("temp dir");
    let game = dir.path().join("installed/tf");
    std::fs::create_dir_all(game.join("models/test")).expect("dirs");
    std::fs::write(game.join(CRATE), mdl_with_hull(Vec3::new(-40.0, -40.0, 0.0), Vec3::new(40.0, 40.0, 100.0)))
        .expect("model");
    let locator = Finds(game.clone(), Default::default());
    let report = generate_nav(&request(loose_map(dir.path(), true)), &locator).expect("generates");
    assert_eq!(report.props.solid, 1);
    assert!(report.props.missing_models.is_empty());
    assert_eq!(*locator.1.borrow(), vec!["tf", "hl2"], "TF2's spec asks for its content folders");
    let located = report.props.located.expect("the search is reported");
    assert_eq!((located.dirs, located.found_by), (vec![game], FoundBy::DefaultPath));
}

#[test]
fn when_the_game_cannot_be_found_the_user_is_told_to_name_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let failure = generate_nav(&request(loose_map(dir.path(), true)), &FindsNothing).expect_err("nothing found");
    assert_eq!(failure.kind, FailureKind::Input);
    assert!(failure.error.to_string().contains("--game-dir"));
}

#[test]
fn a_map_without_solid_props_never_searches_the_machine() {
    let dir = tempfile::tempdir().expect("temp dir");
    let report = generate_nav(&request(loose_map(dir.path(), false)), &MustNotBeAsked).expect("generates");
    assert!(report.props.located.is_none());
}

#[test]
fn named_game_dirs_skip_the_search() {
    let dir = tempfile::tempdir().expect("temp dir");
    let game = dir.path().join("named/tf");
    std::fs::create_dir_all(&game).expect("dirs");
    let mut named = request(loose_map(dir.path(), true));
    named.game_dirs = vec![game];
    let report = generate_nav(&named, &MustNotBeAsked).expect("generates");
    assert_eq!(report.props.missing_models, vec![CRATE.to_string()]);
}
