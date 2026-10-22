//! `navinfo`: analyze a `.nav` and report counts, connectivity, and
//! findings as TOON (default) or JSON on stdout.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, anyhow};
use clap::Parser;
use serde::Serialize;

use crabby_navmesh_builder::analysis::{self, AreaDetail, AreaRow, NavAnalysis};
use crabby_navmesh_builder::cli::{Failure, finish};
use crabby_navmesh_builder::nav::{self, AreaId};
use crabby_navmesh_builder::report::Format;

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
    #[arg(long = "area", value_name = "ID")]
    area_ids: Vec<u32>,
    /// Report in JSON instead of TOON.
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct Report {
    status: &'static str,
    nav: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    bsp: Option<BspCheck>,
    analysis: NavAnalysis,
    #[serde(skip_serializing_if = "Option::is_none")]
    area_list: Option<Vec<AreaRow>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    area_details: Vec<AreaDetail>,
}

#[derive(Serialize)]
struct BspCheck {
    path: String,
    bytes: u64,
    matches: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let format = if args.json { Format::Json } else { Format::Toon };
    finish(run(&args), format)
}

fn run(args: &Args) -> Result<Report, Failure> {
    let bytes =
        std::fs::read(&args.nav).with_context(|| format!("reading {}", args.nav.display())).map_err(Failure::input)?;
    let (file, skipped) =
        nav::decode(&bytes).with_context(|| format!("decoding {}", args.nav.display())).map_err(Failure::input)?;

    let bsp = match &args.bsp {
        None => None,
        Some(path) => {
            let metadata = std::fs::metadata(path)
                .with_context(|| format!("reading {}", path.display()))
                .map_err(Failure::input)?;
            Some(BspCheck {
                path: path.display().to_string(),
                bytes: metadata.len(),
                matches: metadata.len() == u64::from(file.bsp_size),
            })
        }
    };

    let area_details = args
        .area_ids
        .iter()
        .map(|raw| {
            AreaId::new(*raw)
                .and_then(|id| analysis::find_area(&file.areas, id))
                .map(analysis::area_detail)
                .ok_or_else(|| Failure::input(anyhow!("no area has id {raw}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Report {
        status: "ok",
        nav: args.nav.display().to_string(),
        bsp,
        analysis: analysis::analyze(&file, &skipped),
        area_list: args.areas.then(|| analysis::area_rows(&file.areas)),
        area_details,
    })
}
