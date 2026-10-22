//! `navgen`: build the `.nav` for a `.bsp`, and report what happened as
//! TOON (default) or JSON on stdout.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

use crabby_navmesh_builder::app::generate::{GenerateRequest, generate_nav};
use crabby_navmesh_builder::app::report::{Format, finish};
use crabby_navmesh_builder::app::system::SystemLocator;
use crabby_navmesh_builder::app::{Failure, Length};
use crabby_navmesh_builder::core::geom::Vec3;
use crabby_navmesh_builder::spec::Build;

#[derive(Parser)]
#[command(name = "navgen", version, about = "Crabby Navmesh Builder: generate a Source .nav from a .bsp")]
struct Args {
    /// The compiled map.
    bsp: PathBuf,
    /// Output path; defaults to the .bsp's path with a .nav extension.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// The game whose engine will load the mesh: tf, gmod (hl2, css, and csgo are not supported yet).
    #[arg(short, long, default_value = "tf")]
    build: Build,
    /// Extra seed positions as x,y,z, used alongside spawn points.
    #[arg(long, value_parser = parse_vec3)]
    seed: Vec<Vec3>,
    /// Worker threads; defaults to one per core.
    #[arg(short = 'j', long)]
    threads: Option<usize>,
    /// Grid spacing in units (default 25).
    #[arg(long)]
    cell_size: Option<Length>,
    /// Largest height a cell corner may stray from its merged area (default 6).
    #[arg(long)]
    merge_tolerance: Option<Length>,
    /// Leave static props out, so players walk through them.
    #[arg(long)]
    no_props: bool,
    /// A game directory to find prop models in (loose files and *_dir.vpk); repeatable.
    /// Defaults to the game holding the map's maps/; failing that, the game is searched for
    /// (a cached Steam home, default install paths, running launchers), then asked for.
    #[arg(long = "game-dir", value_name = "DIR")]
    game_dirs: Vec<PathBuf>,
    /// Report in JSON instead of TOON.
    #[arg(long)]
    json: bool,
}

fn parse_vec3(text: &str) -> Result<Vec3, String> {
    let parts: Vec<f32> = text
        .split(',')
        .map(|p| p.trim().parse::<f32>().map_err(|e| format!("{p:?}: {e}")))
        .collect::<Result<_, _>>()?;
    match parts[..] {
        [x, y, z] => Ok(Vec3::new(x, y, z)),
        _ => Err(format!("expected x,y,z, got {text:?}")),
    }
}

fn main() -> ExitCode {
    let args = Args::parse();
    let format = if args.json { Format::Json } else { Format::Toon };
    let request = GenerateRequest {
        bsp: args.bsp,
        output: args.output,
        build: args.build,
        seeds: args.seed,
        cell_size: args.cell_size,
        merge_tolerance: args.merge_tolerance,
        props: !args.no_props,
        game_dirs: args.game_dirs,
    };
    let mut pool = rayon::ThreadPoolBuilder::new();
    if let Some(n) = args.threads {
        pool = pool.num_threads(n);
    }
    let result = match pool.build().context("starting worker threads") {
        Ok(pool) => pool.install(|| generate_nav(&request, &SystemLocator::new())),
        Err(e) => Err(Failure::internal(e)),
    };
    finish(result, format)
}
