//! `navgen2`'s use case: what `nav_analyze` adds to a mesh, computed from
//! its map without the game. This first milestone finds hiding spots and
//! each area's visible set, what TF2's bots read most.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, anyhow};
use serde::Serialize;

use crate::core::collision::SolidGrid;
use crate::core::hiding::hiding_spots;
use crate::core::mesh::{HidingFlags, NavMesh, Visibility};
use crate::core::visibility::visible_sets;
use crate::core::world::{Contents, PropModels, World};
use crate::format::{bsp, nav};
use crate::spec;

use super::Failure;
use super::content::game_content;
use super::models::ModelLibrary;

/// The `nav_analyze` steps this milestone leaves to the game.
const STEPS_NOT_DONE: [&str; 4] = ["encounter_spots", "sniper_spots", "earliest_occupy_times", "light_intensity"];

#[derive(Clone, Debug)]
pub struct AnalyzeRequest {
    pub nav: PathBuf,
    /// Defaults to the `.nav`'s path with a `.bsp` extension.
    pub bsp: Option<PathBuf>,
    /// Defaults to rewriting the `.nav` in place, as `nav_analyze` does.
    pub output: Option<PathBuf>,
    /// Whether static props block sight and give cover.
    pub props: bool,
    pub game_dirs: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeReport {
    pub status: &'static str,
    pub build: String,
    pub nav: NavReport,
    pub bsp: BspReport,
    pub hiding_spots: HidingReport,
    pub visibility: VisibilityReport,
    pub props: PropsReport,
    pub steps_done: Vec<&'static str>,
    pub steps_not_done: Vec<&'static str>,
    pub threads: usize,
    pub elapsed_ms: ElapsedReport,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct NavReport {
    pub input: String,
    pub output: String,
    pub areas: usize,
    pub bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct BspReport {
    pub path: String,
    /// Whether the map is still the one the mesh was built from.
    pub matches: bool,
}

#[derive(Debug, Serialize)]
pub struct HidingReport {
    pub total: usize,
    pub in_cover: usize,
    pub exposed: usize,
    pub areas_with_spots: usize,
}

#[derive(Debug, Serialize)]
pub struct VisibilityReport {
    pub entries: usize,
    pub completely: usize,
    pub potentially_only: usize,
    pub mean_per_area: f64,
}

#[derive(Debug, Serialize)]
pub struct PropsReport {
    pub placed: bool,
    pub solid: usize,
    pub missing_models: usize,
}

#[derive(Debug, Serialize)]
pub struct ElapsedReport {
    pub hiding_spots: u128,
    pub visibility: u128,
    pub total: u128,
}

pub fn analyze_nav(request: &AnalyzeRequest) -> Result<AnalyzeReport, Failure> {
    let started = Instant::now();
    let nav_bytes = std::fs::read(&request.nav)
        .with_context(|| format!("reading {}", request.nav.display()))
        .map_err(Failure::input)?;
    let parsed = nav::parse(&nav_bytes, |sub_version| spec::for_sub_version(sub_version).map(|s| s.dialect()))
        .with_context(|| format!("parsing {}", request.nav.display()))
        .map_err(Failure::input)?;
    let spec = spec::for_sub_version(parsed.sub_version).expect("the parser accepted only a known sub-version");

    let bsp_path = request.bsp.clone().unwrap_or_else(|| request.nav.with_extension("bsp"));
    let bsp_bytes = std::fs::read(&bsp_path)
        .with_context(|| format!("reading {}, the map the mesh belongs to", bsp_path.display()))
        .map_err(Failure::input)?;
    let map =
        bsp::parse(&bsp_bytes).with_context(|| format!("parsing {}", bsp_path.display())).map_err(Failure::input)?;
    let matches = u32::try_from(bsp_bytes.len()).is_ok_and(|n| n == parsed.mesh.bsp_size);

    let library = if request.props {
        let (search, _) = game_content(&bsp_bytes, &bsp_path, &request.game_dirs)?;
        Some(ModelLibrary::load(&search, map.contents.props()))
    } else {
        None
    };
    let models = library.as_ref().map(|l| l as &dyn PropModels);
    let world_for = |mask| -> Result<(SolidGrid, World), Failure> {
        let world = World::assemble(&map.contents, spec.policy(), models, mask);
        let grid = SolidGrid::new(world.solids.clone()).map_err(Failure::input)?;
        Ok((grid, world))
    };
    let (cover, _) = world_for(Contents::NPC_SOLID)?;
    let (sight, sight_world) = world_for(Contents::SIGHT)?;

    let hiding_started = Instant::now();
    let spots = hiding_spots(&parsed.mesh, &cover);
    let hiding_ms = hiding_started.elapsed().as_millis();
    let visibility_started = Instant::now();
    let sets = visible_sets(&parsed.mesh, &sight);
    let visibility_ms = visibility_started.elapsed().as_millis();

    let areas = parsed
        .mesh
        .areas
        .into_iter()
        .zip(spots)
        .zip(sets)
        .map(|((area, spots), set)| area.with_hiding_spots(spots).with_visible_areas(set))
        .collect::<Vec<_>>();
    let mesh = NavMesh { bsp_size: parsed.mesh.bsp_size, analyzed: true, areas };
    let compiled = nav::compile(&mesh, spec.dialect());
    let output = request.output.clone().unwrap_or_else(|| request.nav.clone());
    write_replacing(&output, &compiled)?;

    let all_spots = || mesh.areas.iter().flat_map(|a| a.hiding_spots().iter());
    let entries = || mesh.areas.iter().flat_map(|a| a.visible_areas().iter());
    let entry_count = entries().count();
    let completely = entries().filter(|v| v.visibility.bits() & Visibility::COMPLETELY.bits() != 0).count();
    Ok(AnalyzeReport {
        status: "ok",
        build: spec.build().to_string(),
        nav: NavReport {
            input: request.nav.display().to_string(),
            output: output.display().to_string(),
            areas: mesh.areas.len(),
            bytes: compiled.len(),
        },
        bsp: BspReport { path: bsp_path.display().to_string(), matches },
        hiding_spots: HidingReport {
            total: all_spots().count(),
            in_cover: all_spots().filter(|s| s.flags.contains(HidingFlags::IN_COVER)).count(),
            exposed: all_spots().filter(|s| s.flags.contains(HidingFlags::EXPOSED)).count(),
            areas_with_spots: mesh.areas.iter().filter(|a| !a.hiding_spots().is_empty()).count(),
        },
        visibility: VisibilityReport {
            entries: entry_count,
            completely,
            potentially_only: entry_count - completely,
            mean_per_area: entry_count as f64 / mesh.areas.len().max(1) as f64,
        },
        props: PropsReport {
            placed: sight_world.props.placed,
            solid: sight_world.props.solid,
            missing_models: sight_world.props.missing_models.len(),
        },
        steps_done: vec!["hiding_spots", "visibility"],
        steps_not_done: STEPS_NOT_DONE.to_vec(),
        threads: rayon::current_num_threads(),
        elapsed_ms: ElapsedReport {
            hiding_spots: hiding_ms,
            visibility: visibility_ms,
            total: started.elapsed().as_millis(),
        },
        next_steps: vec![format!("navinfo {}", output.display())],
    })
}

/// Writes beside the destination, then renames over it, so a failure
/// never leaves half a mesh where the old one was.
fn write_replacing(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let file_name = path.file_name().ok_or_else(|| Failure::input(anyhow!("{} is not a file path", path.display())))?;
    let partial = path.with_file_name(format!(".{}.partial", file_name.to_string_lossy()));
    std::fs::write(&partial, bytes)
        .with_context(|| format!("writing {}", partial.display()))
        .map_err(Failure::internal)?;
    std::fs::rename(&partial, path).with_context(|| format!("replacing {}", path.display())).map_err(Failure::internal)
}
