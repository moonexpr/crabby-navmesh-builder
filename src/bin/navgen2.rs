//! `navgen2`: add what `nav_analyze` adds to a `.nav` (hiding spots and
//! visibility, for now), from its map, and report as TOON or JSON.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

use crabby_navmesh_builder::app::Failure;
use crabby_navmesh_builder::app::analyze::{AnalyzeRequest, analyze_nav};
use crabby_navmesh_builder::app::report::{Format, finish};

#[derive(Parser)]
#[command(name = "navgen2", version, about = "Crabby Navmesh Builder: analyze a Source .nav as nav_analyze does")]
struct Args {
    /// The mesh to analyze.
    nav: PathBuf,
    /// Its map; defaults to the .nav's path with a .bsp extension.
    #[arg(long)]
    bsp: Option<PathBuf>,
    /// Where to write the analyzed mesh; defaults to replacing the input, as nav_analyze does.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Leave static props out, so they neither block sight nor give cover.
    #[arg(long)]
    no_props: bool,
    /// A game directory to find prop models in (loose files and *_dir.vpk); repeatable.
    #[arg(long = "game-dir", value_name = "DIR")]
    game_dirs: Vec<PathBuf>,
    /// Worker threads; defaults to one per core.
    #[arg(short = 'j', long)]
    threads: Option<usize>,
    /// Report in JSON instead of TOON.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let format = if args.json { Format::Json } else { Format::Toon };
    let request = AnalyzeRequest {
        nav: args.nav,
        bsp: args.bsp,
        output: args.output,
        props: !args.no_props,
        game_dirs: args.game_dirs,
    };
    let mut pool = rayon::ThreadPoolBuilder::new();
    if let Some(n) = args.threads {
        pool = pool.num_threads(n);
    }
    let result = match pool.build().context("starting worker threads") {
        Ok(pool) => pool.install(|| analyze_nav(&request)),
        Err(e) => Err(Failure::internal(e)),
    };
    finish(result, format)
}
