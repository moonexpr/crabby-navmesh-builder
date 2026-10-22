//! `navinfo`'s use case: parse a `.nav` of any supported build and report on it.

use std::path::PathBuf;

use anyhow::{Context, anyhow};
use serde::Serialize;

use crate::core::analysis::{self, AreaDetail, AreaRow, NavAnalysis};
use crate::core::mesh::AreaId;
use crate::format::nav::{self, NavDialect, Skipped};
use crate::spec;

use super::Failure;

/// What to report, already parsed from the command line.
#[derive(Clone, Debug)]
pub struct InspectRequest {
    pub nav: PathBuf,
    /// The map the mesh was built from, to check its stored size.
    pub bsp: Option<PathBuf>,
    /// Include every area as a table row.
    pub areas: bool,
    /// Areas to include in full.
    pub area_ids: Vec<AreaId>,
}

#[derive(Debug, Serialize)]
pub struct InspectReport {
    pub status: &'static str,
    pub nav: String,
    pub format: FormatReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bsp: Option<BspCheck>,
    pub analysis: NavAnalysis,
    pub skipped: SkippedReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area_list: Option<Vec<AreaRow>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub area_details: Vec<AreaDetail>,
}

#[derive(Debug, Serialize)]
pub struct FormatReport {
    pub build: String,
    pub version: u32,
    pub sub_version: u32,
    pub analyzed: bool,
    pub bsp_size: u32,
}

#[derive(Debug, Serialize)]
pub struct BspCheck {
    pub path: String,
    pub bytes: u64,
    pub matches: bool,
}

#[derive(Debug, Serialize)]
pub struct SkippedReport {
    pub places: usize,
    pub hiding_spots: usize,
    pub encounter_paths: usize,
    pub ladders: usize,
    pub visible_area_entries: usize,
}

impl From<&Skipped> for SkippedReport {
    fn from(s: &Skipped) -> Self {
        Self {
            places: s.places,
            hiding_spots: s.hiding_spots,
            encounter_paths: s.encounter_paths,
            ladders: s.ladders,
            visible_area_entries: s.visible_area_entries,
        }
    }
}

fn dialect_for(sub_version: u32) -> Option<&'static dyn NavDialect> {
    spec::for_sub_version(sub_version).map(|s| s.dialect())
}

pub fn inspect_nav(request: &InspectRequest) -> Result<InspectReport, Failure> {
    let bytes = std::fs::read(&request.nav)
        .with_context(|| format!("reading {}", request.nav.display()))
        .map_err(Failure::input)?;
    let parsed = nav::parse(&bytes, dialect_for)
        .with_context(|| format!("parsing {}", request.nav.display()))
        .map_err(Failure::input)?;
    let build =
        spec::for_sub_version(parsed.sub_version).expect("the parser accepted only a known sub-version").build();

    let bsp = match &request.bsp {
        None => None,
        Some(path) => {
            let bytes = std::fs::metadata(path)
                .with_context(|| format!("reading {}", path.display()))
                .map_err(Failure::input)?
                .len();
            Some(BspCheck {
                path: path.display().to_string(),
                bytes,
                matches: bytes == u64::from(parsed.mesh.bsp_size),
            })
        }
    };

    let area_details = request
        .area_ids
        .iter()
        .map(|id| {
            analysis::find_area(&parsed.mesh.areas, *id)
                .map(analysis::area_detail)
                .ok_or_else(|| Failure::input(anyhow!("no area has id {}", id.get())))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(InspectReport {
        status: "ok",
        nav: request.nav.display().to_string(),
        format: FormatReport {
            build: build.to_string(),
            version: nav::VERSION,
            sub_version: parsed.sub_version,
            analyzed: parsed.mesh.analyzed,
            bsp_size: parsed.mesh.bsp_size,
        },
        bsp,
        analysis: analysis::analyze(&parsed.mesh),
        skipped: SkippedReport::from(&parsed.skipped),
        area_list: request.areas.then(|| analysis::area_rows(&parsed.mesh.areas)),
        area_details,
    })
}
