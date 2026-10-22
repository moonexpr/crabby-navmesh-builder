mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crabby_navmesh_builder::core::geom::Vec3;
use serde_json::Value;

use support::bsp_writer::{BspWriter, CONTENTS_SOLID, SOLID_BBOX};
use support::mdl_with_hull;

/// A walled 512-unit room with one spawn point.
fn room_bsp(dir: &Path) -> PathBuf {
    let mut map = BspWriter::new();
    map.box_brush(0, Vec3::new(0.0, 0.0, -16.0), Vec3::new(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.entity(&[("classname", "info_player_teamspawn"), ("origin", "256 256 8")]);
    let path = dir.join("room.bsp");
    std::fs::write(&path, map.build(20)).expect("fixture written");
    path
}

fn run(binary: &str, args: &[&str]) -> Output {
    Command::new(binary).args(args).output().expect("binary runs")
}

fn toon(output: &Output) -> Value {
    let text = String::from_utf8(output.stdout.clone()).expect("utf-8");
    toon_format::decode_default(&text).unwrap_or_else(|e| panic!("stdout is TOON ({e}):\n{text}"))
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout is JSON")
}

const NAVGEN: &str = env!("CARGO_BIN_EXE_navgen");
const NAVINFO: &str = env!("CARGO_BIN_EXE_navinfo");

#[test]
fn navgen_writes_the_nav_and_reports_in_toon() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8 path")]);
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let report = toon(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["nav"]["areas"], 1);
    assert_eq!(report["seeds"]["spawn_points"], 1);
    let nav = dir.path().join("room.nav");
    assert_eq!(report["nav"]["path"], nav.to_str().expect("utf-8 path"));
    assert!(nav.exists());
}

#[test]
fn navgen_reports_in_json_when_asked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    let out = dir.path().join("custom.nav");
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8"), "-o", out.to_str().expect("utf-8"), "--json"]);
    assert!(output.status.success());
    let report = json(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["mesh"]["areas"], 1);
    assert!(out.exists());
}

#[test]
fn navgen_reports_bad_input_with_exit_code_2() {
    let dir = tempfile::tempdir().expect("temp dir");
    let junk = dir.path().join("junk.bsp");
    std::fs::write(&junk, b"definitely not a map").expect("written");
    let output = run(NAVGEN, &[junk.to_str().expect("utf-8")]);
    assert_eq!(output.status.code(), Some(2));
    let report = toon(&output);
    assert_eq!(report["status"], "error");
    assert_eq!(report["kind"], "input");
}

#[test]
fn navgen_reports_a_missing_file_as_input() {
    let output = run(NAVGEN, &["/nonexistent/dir/map.bsp", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json(&output)["kind"], "input");
}

#[test]
fn navinfo_analyzes_a_generated_mesh() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    assert!(run(NAVGEN, &[bsp.to_str().expect("utf-8")]).status.success());
    let nav = dir.path().join("room.nav");
    let output = run(NAVINFO, &[nav.to_str().expect("utf-8"), "--bsp", bsp.to_str().expect("utf-8")]);
    assert!(output.status.success(), "stdout: {}", String::from_utf8_lossy(&output.stdout));
    let report = toon(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["analysis"]["areas"]["count"], 1);
    assert_eq!(report["analysis"]["components"]["count"], 1);
    assert_eq!(report["bsp"]["matches"], true);
}

#[test]
fn navinfo_lists_areas_as_a_table_and_details_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    assert!(run(NAVGEN, &[bsp.to_str().expect("utf-8")]).status.success());
    let nav = dir.path().join("room.nav");
    let output = run(NAVINFO, &[nav.to_str().expect("utf-8"), "--areas", "--area", "1"]);
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout.clone()).expect("utf-8");
    assert!(text.contains("area_list[1]{id,"), "tabular TOON rows:\n{text}");
    let report = toon(&output);
    assert_eq!(report["area_list"][0]["id"], 1);
    assert_eq!(report["area_details"][0]["id"], 1);
}

#[test]
fn navinfo_reports_an_unknown_area_as_input() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    assert!(run(NAVGEN, &[bsp.to_str().expect("utf-8")]).status.success());
    let nav = dir.path().join("room.nav");
    let output = run(NAVINFO, &[nav.to_str().expect("utf-8"), "--area", "99", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(json(&output)["message"].as_str().expect("message").contains("99"));
}

#[test]
fn navinfo_reports_a_non_nav_file_as_input() {
    let dir = tempfile::tempdir().expect("temp dir");
    let junk = dir.path().join("junk.nav");
    std::fs::write(&junk, b"nope nope nope").expect("written");
    let output = run(NAVINFO, &[junk.to_str().expect("utf-8")]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(toon(&output)["kind"], "input");
}

#[test]
fn navgen_targets_tf_unless_told_otherwise() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    let report = json(&run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--json"]));
    assert_eq!(report["build"], "tf");
    assert_eq!(report["nav"]["sub_version"], 2);
}

#[test]
fn navgen_writes_the_gmod_dialect_that_navinfo_recognizes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8"), "-b", "gmod", "--json"]);
    assert!(output.status.success(), "stdout: {}", String::from_utf8_lossy(&output.stdout));
    let report = json(&output);
    assert_eq!(report["build"], "gmod");
    assert_eq!(report["nav"]["sub_version"], 0);
    let nav = dir.path().join("room.nav");
    let info = json(&run(NAVINFO, &[nav.to_str().expect("utf-8"), "--json"]));
    assert_eq!(info["format"]["build"], "gmod");
    assert_eq!(info["format"]["sub_version"], 0);
    assert_eq!(info["analysis"]["areas"]["count"], 1);
}

#[test]
fn navgen_refuses_a_build_that_is_not_supported_yet_as_input() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--build", "css"]);
    assert_eq!(output.status.code(), Some(2));
    let report = toon(&output);
    assert_eq!(report["kind"], "input");
    assert!(report["message"].as_str().expect("message").contains("not supported yet"));
    assert!(!dir.path().join("room.nav").exists(), "nothing is written for a refused build");
}

#[test]
fn navgen_refuses_a_cell_size_that_is_not_a_positive_distance() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = room_bsp(dir.path());
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--cell-size", "0"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("positive distance"));
}

const CRATE: &str = "models/test/crate.mdl";

/// `<root>/tf/maps/yard.bsp`: a 512-unit floor, a spawn, and an 80-unit
/// crate prop at (300, 300) whose model lives where `place_model` puts it.
fn yard(root: &Path, place_model: impl FnOnce(&Path, &mut BspWriter)) -> PathBuf {
    let game = root.join("tf");
    std::fs::create_dir_all(game.join("maps")).expect("dirs");
    let mut map = BspWriter::new();
    map.box_brush(0, Vec3::new(0.0, 0.0, -16.0), Vec3::new(512.0, 512.0, 0.0), CONTENTS_SOLID);
    map.entity(&[("classname", "info_player_teamspawn"), ("origin", "100 100 8")]);
    map.static_prop(CRATE, Vec3::new(300.0, 300.0, 0.0), [0.0, 0.0, 0.0], SOLID_BBOX);
    place_model(&game, &mut map);
    let path = game.join("maps/yard.bsp");
    std::fs::write(&path, map.build(20)).expect("fixture written");
    path
}

fn crate_model() -> Vec<u8> {
    mdl_with_hull(Vec3::new(-40.0, -40.0, 0.0), Vec3::new(40.0, 40.0, 100.0))
}

fn loose_crate(game: &Path, _: &mut BspWriter) {
    std::fs::create_dir_all(game.join("models/test")).expect("dirs");
    std::fs::write(game.join(CRATE), crate_model()).expect("written");
}

/// Whether any area of the mesh at `nav` covers the point in xy.
fn covered(nav: &Path, x: f64, y: f64) -> bool {
    let info = json(&run(NAVINFO, &[nav.to_str().expect("utf-8"), "--areas", "--json"]));
    info["area_list"].as_array().expect("rows").iter().any(|row| {
        let f = |k: &str| row[k].as_f64().expect("number");
        f("min_x") <= x && x <= f("max_x") && f("min_y") <= y && y <= f("max_y")
    })
}

#[test]
fn a_solid_prop_from_the_game_dir_cuts_a_hole_in_the_mesh() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = yard(dir.path(), loose_crate);
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--json"]);
    assert!(output.status.success(), "stdout: {}", String::from_utf8_lossy(&output.stdout));
    let report = json(&output);
    assert_eq!(report["props"]["placed"], true);
    assert_eq!(report["props"]["solid"], 1);
    assert_eq!(report["props"]["missing_models"].as_array().expect("list").len(), 0);
    let nav = bsp.with_extension("nav");
    assert!(!covered(&nav, 300.0, 300.0), "the crate's footprint is not walkable");
    assert!(covered(&nav, 100.0, 100.0));
}

#[test]
fn without_props_the_floor_under_the_crate_is_walkable() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = yard(dir.path(), loose_crate);
    let report = json(&run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--no-props", "--json"]));
    assert_eq!(report["props"]["placed"], false);
    assert!(covered(&bsp.with_extension("nav"), 300.0, 300.0));
}

#[test]
fn a_model_packed_in_the_map_is_found_without_a_game_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = yard(dir.path(), |_, map| map.pack(CRATE, &crate_model()));
    let report = json(&run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--json"]));
    assert_eq!(report["props"]["solid"], 1);
    assert!(!covered(&bsp.with_extension("nav"), 300.0, 300.0));
}

#[test]
fn a_model_found_nowhere_is_reported_missing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let bsp = yard(dir.path(), |_, _| {});
    let output = run(NAVGEN, &[bsp.to_str().expect("utf-8"), "--json"]);
    assert!(output.status.success(), "a missing model is reported, not fatal");
    let report = json(&output);
    assert_eq!(report["props"]["solid"], 0);
    assert_eq!(report["props"]["missing_models"][0], CRATE);
}
