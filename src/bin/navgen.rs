//! `navgen`: build the `.nav` for a `.bsp`, and report what happened as
//! TOON (default) or JSON on stdout.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use serde::Serialize;

use crabby_navmesh_builder::bsp::{self, Entity};
use crabby_navmesh_builder::cli::{Failure, finish};
use crabby_navmesh_builder::collision::SolidGrid;
use crabby_navmesh_builder::generate::{self, Params};
use crabby_navmesh_builder::geom::Vec3;
use crabby_navmesh_builder::nav::{self, NavFile};
use crabby_navmesh_builder::report::Format;

/// Classnames whose origins seed the walkable flood fill.
const SPAWN_CLASSES: [&str; 7] = [
    "info_player_teamspawn",
    "info_player_start",
    "info_player_deathmatch",
    "info_player_combine",
    "info_player_rebel",
    "info_player_terrorist",
    "info_player_counterterrorist",
];

#[derive(Parser)]
#[command(name = "navgen", version, about = "Crabby Navmesh Builder: generate a Source .nav from a .bsp")]
struct Args {
    /// The compiled map.
    bsp: PathBuf,
    /// Output path; defaults to the .bsp's path with a .nav extension.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Extra seed positions as x,y,z, used alongside spawn points.
    #[arg(long, value_parser = parse_vec3)]
    seed: Vec<Vec3>,
    /// Worker threads; defaults to one per core.
    #[arg(short = 'j', long)]
    threads: Option<usize>,
    /// Grid spacing in units.
    #[arg(long, default_value_t = Params::default().cell_size)]
    cell_size: f32,
    /// Largest height a cell corner may stray from its merged area.
    #[arg(long, default_value_t = Params::default().merge_tolerance)]
    merge_tolerance: f32,
    /// Report in JSON instead of TOON.
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct Report {
    status: &'static str,
    bsp: BspReport,
    nav: NavReport,
    map: MapReport,
    mesh: MeshReport,
    seeds: SeedReport,
    threads: usize,
    elapsed_ms: u128,
    next_steps: Vec<String>,
}

#[derive(Serialize)]
struct BspReport {
    path: String,
    version: u32,
    bytes: u32,
}

#[derive(Serialize)]
struct NavReport {
    path: String,
    bytes: usize,
    areas: usize,
}

#[derive(Serialize)]
struct MapReport {
    solids: usize,
    brushes: usize,
    brush_entities: usize,
    displacements: usize,
    displacement_triangles: usize,
}

#[derive(Serialize)]
struct MeshReport {
    columns: usize,
    samples: usize,
    reachable_samples: usize,
    cells: usize,
    areas: usize,
}

#[derive(Serialize)]
struct SeedReport {
    spawn_points: usize,
    extra: usize,
    used: usize,
    off_mesh: Vec<[f32; 3]>,
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
    finish(run(&args), format)
}

fn run(args: &Args) -> Result<Report, Failure> {
    let params = Params { cell_size: args.cell_size, merge_tolerance: args.merge_tolerance, ..Params::default() };
    let output = args.output.clone().unwrap_or_else(|| args.bsp.with_extension("nav"));
    let mut pool = rayon::ThreadPoolBuilder::new();
    if let Some(n) = args.threads {
        pool = pool.num_threads(n);
    }
    let pool = pool.build().context("starting worker threads").map_err(Failure::internal)?;
    pool.install(|| build(&args.bsp, &output, &args.seed, &params))
}

fn build(bsp_path: &Path, output: &Path, extra_seeds: &[Vec3], params: &Params) -> Result<Report, Failure> {
    let started = Instant::now();
    let bytes =
        std::fs::read(bsp_path).with_context(|| format!("reading {}", bsp_path.display())).map_err(Failure::input)?;
    let bsp_size = u32::try_from(bytes.len()).context("the .bsp is over 4 GiB").map_err(Failure::input)?;
    let map = bsp::parse(&bytes).with_context(|| format!("parsing {}", bsp_path.display())).map_err(Failure::input)?;

    let spawns = spawn_points(&map.entities);
    let seeds: Vec<Vec3> = spawns.iter().chain(extra_seeds).copied().collect();
    let map_report = MapReport {
        solids: map.solids.len(),
        brushes: map.stats.brushes,
        brush_entities: map.stats.brush_entities,
        displacements: map.stats.displacements,
        displacement_triangles: map.stats.displacement_triangles,
    };
    let world = SolidGrid::new(map.solids).map_err(Failure::input)?;
    let generated = generate::generate(&world, &seeds, params).map_err(Failure::input)?;
    let stats = generated.stats;
    if generated.areas.is_empty() {
        return Err(Failure::input(anyhow::anyhow!("no walkable areas are reachable from the seeds")));
    }

    let file = NavFile { bsp_size, analyzed: false, areas: generated.areas };
    let encoded = nav::encode(&file);
    std::fs::write(output, &encoded)
        .with_context(|| format!("writing {}", output.display()))
        .map_err(Failure::internal)?;

    Ok(Report {
        status: "ok",
        bsp: BspReport { path: bsp_path.display().to_string(), version: map.version, bytes: bsp_size },
        nav: NavReport { path: output.display().to_string(), bytes: encoded.len(), areas: file.areas.len() },
        map: map_report,
        mesh: MeshReport {
            columns: stats.columns,
            samples: stats.samples,
            reachable_samples: stats.reachable_samples,
            cells: stats.cells,
            areas: stats.areas,
        },
        seeds: SeedReport {
            spawn_points: spawns.len(),
            extra: extra_seeds.len(),
            used: stats.seeds_used,
            off_mesh: stats.seeds_off_mesh.iter().map(|v| v.to_array()).collect(),
        },
        threads: rayon::current_num_threads(),
        elapsed_ms: started.elapsed().as_millis(),
        next_steps: vec![
            format!("navinfo {}", output.display()),
            "in game: nav_analyze, to add hiding spots, encounter paths, and visibility".to_string(),
        ],
    })
}

fn spawn_points(entities: &[Entity]) -> Vec<Vec3> {
    entities.iter().filter(|e| SPAWN_CLASSES.contains(&e.classname.as_str())).map(|e| e.origin).collect()
}
