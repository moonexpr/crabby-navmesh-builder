//! `navinfo`: analyze a `.nav` of any supported build, and report counts,
//! connectivity, and findings as TOON (default) or JSON on stdout.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crabby_navmesh_builder::app::inspect::{InspectRequest, inspect_nav};
use crabby_navmesh_builder::app::report::{Format, finish};
use crabby_navmesh_builder::core::mesh::AreaId;

#[derive(Parser)]
#[command(name = "navinfo", version, about = "Crabby Navmesh Builder: analyze a Source .nav")]
struct Args {
    /// The navigation mesh.
    nav: PathBuf,
    /// The map it was built from, to check the stored size still matches.
    #[arg(long)]
    bsp: Option<PathBuf>,
    /// Include every area as a table row.
    #[arg(long)]
    areas: bool,
    /// Include one area in full, with its connections; repeatable.
    #[arg(long = "area", value_name = "ID", value_parser = parse_area_id)]
    area_ids: Vec<AreaId>,
    /// Report in JSON instead of TOON.
    #[arg(long)]
    json: bool,
}

fn parse_area_id(text: &str) -> Result<AreaId, String> {
    let n: u32 = text.parse().map_err(|e| format!("{text:?}: {e}"))?;
    AreaId::new(n).ok_or_else(|| "area ids start at 1".to_string())
}

fn main() -> ExitCode {
    let args = Args::parse();
    let format = if args.json { Format::Json } else { Format::Toon };
    let request = InspectRequest { nav: args.nav, bsp: args.bsp, areas: args.areas, area_ids: args.area_ids };
    finish(inspect_nav(&request), format)
}
