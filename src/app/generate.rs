//! `navgen`'s use case: build a game's `.nav` for a `.bsp`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, anyhow};
use serde::Serialize;

use crate::core::collision::SolidGrid;
use crate::core::generate::{self, Params};
use crate::core::geom::Vec3;
use crate::core::mesh::NavMesh;
use crate::core::world::{Contents, PropModels, World};
use crate::format::{bsp, nav};
use crate::spec::Build;

use super::content::{Pakfile, SearchPath, mount_game_dir};
use super::models::{BrokenModel, ModelLibrary};
use super::{Failure, Length};

/// What to build, already parsed from the command line.
#[derive(Clone, Debug)]
pub struct GenerateRequest {
    pub bsp: PathBuf,
    /// Defaults to the `.bsp`'s path with a `.nav` extension.
    pub output: Option<PathBuf>,
    pub build: Build,
    /// Extra flood-fill starts, used alongside the map's spawn points.
    pub seeds: Vec<Vec3>,
    pub cell_size: Option<Length>,
    pub merge_tolerance: Option<Length>,
    /// Whether static props block players; off, they are left out.
    pub props: bool,
    /// Game directories to find prop models in, such as `tf/`. Empty means
    /// the one holding the map's `maps/`, and an `hl2/` beside it.
    pub game_dirs: Vec<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct GenerateReport {
    pub status: &'static str,
    pub build: String,
    pub bsp: BspReport,
    pub nav: NavReport,
    pub map: MapReport,
    pub mesh: MeshReport,
    pub seeds: SeedReport,
    pub props: PropReport,
    pub threads: usize,
    pub elapsed_ms: u128,
    pub next_steps: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BspReport {
    pub path: String,
    pub version: u32,
    pub bytes: u32,
}

#[derive(Debug, Serialize)]
pub struct NavReport {
    pub path: String,
    pub sub_version: u32,
    pub bytes: usize,
    pub areas: usize,
}

#[derive(Debug, Serialize)]
pub struct MapReport {
    pub solids: usize,
    pub brushes: usize,
    pub brush_models: usize,
    pub solid_brush_entities: usize,
    pub displacements: usize,
    pub displacement_triangles: usize,
}

#[derive(Debug, Serialize)]
pub struct PropReport {
    pub placed: bool,
    pub total: usize,
    pub solid: usize,
    pub non_solid: usize,
    /// Models solid props name that no content source holds.
    pub missing_models: Vec<String>,
    /// Models a source holds but that could not be read.
    pub broken_models: Vec<BrokenModel>,
    /// Where models were looked for, in order.
    pub content: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MeshReport {
    pub columns: usize,
    pub samples: usize,
    pub reachable_samples: usize,
    pub cells: usize,
    pub areas: usize,
}

#[derive(Debug, Serialize)]
pub struct SeedReport {
    pub spawn_points: usize,
    pub extra: usize,
    pub used: usize,
    pub off_mesh: Vec<[f32; 3]>,
}

/// Runs on the current rayon pool; the composition root sizes it.
pub fn generate_nav(request: &GenerateRequest) -> Result<GenerateReport, Failure> {
    let started = Instant::now();
    let spec = request.build.spec().map_err(Failure::input)?;
    let params = Params {
        cell_size: request.cell_size.map_or(spec.params().cell_size, Length::get),
        merge_tolerance: request.merge_tolerance.map_or(spec.params().merge_tolerance, Length::get),
        ..spec.params()
    };
    let output = request.output.clone().unwrap_or_else(|| request.bsp.with_extension("nav"));

    let bytes = std::fs::read(&request.bsp)
        .with_context(|| format!("reading {}", request.bsp.display()))
        .map_err(Failure::input)?;
    let bsp_size = u32::try_from(bytes.len()).context("the .bsp is over 4 GiB").map_err(Failure::input)?;
    let map =
        bsp::parse(&bytes).with_context(|| format!("parsing {}", request.bsp.display())).map_err(Failure::input)?;

    let (library, content) = if request.props {
        let (search, content) = game_content(&bytes, &request.bsp, &request.game_dirs)?;
        (Some(ModelLibrary::load(&search, map.contents.props())), content)
    } else {
        (None, Vec::new())
    };
    let world = World::assemble(
        &map.contents,
        spec.policy(),
        library.as_ref().map(|l| l as &dyn PropModels),
        Contents::PLAYER_SOLID,
    );
    let prop_report = PropReport {
        placed: world.props.placed,
        total: world.props.total,
        solid: world.props.solid,
        non_solid: world.props.non_solid,
        missing_models: world.props.missing_models.clone(),
        broken_models: library.as_ref().map(|l| l.broken().to_vec()).unwrap_or_default(),
        content,
    };
    let seeds: Vec<Vec3> = world.spawns.iter().chain(&request.seeds).copied().collect();
    let map_report = MapReport {
        solids: world.solids.len(),
        brushes: map.stats.brushes,
        brush_models: map.stats.brush_models,
        solid_brush_entities: world.solid_brush_entities,
        displacements: map.stats.displacements,
        displacement_triangles: map.stats.displacement_triangles,
    };
    let spawn_points = world.spawns.len();
    let grid = SolidGrid::new(world.solids).map_err(Failure::input)?;
    let generated = generate::generate(&grid, &seeds, &params).map_err(Failure::input)?;
    if generated.areas.is_empty() {
        return Err(Failure::input(anyhow!("no walkable areas are reachable from the seeds")));
    }

    let stats = generated.stats;
    let mesh = NavMesh { bsp_size, analyzed: false, areas: generated.areas };
    let compiled = nav::compile(&mesh, spec.dialect());
    std::fs::write(&output, &compiled)
        .with_context(|| format!("writing {}", output.display()))
        .map_err(Failure::internal)?;

    Ok(GenerateReport {
        status: "ok",
        build: spec.build().to_string(),
        bsp: BspReport { path: request.bsp.display().to_string(), version: map.version, bytes: bsp_size },
        nav: NavReport {
            path: output.display().to_string(),
            sub_version: spec.dialect().sub_version(),
            bytes: compiled.len(),
            areas: mesh.areas.len(),
        },
        map: map_report,
        mesh: MeshReport {
            columns: stats.columns,
            samples: stats.samples,
            reachable_samples: stats.reachable_samples,
            cells: stats.cells,
            areas: stats.areas,
        },
        seeds: SeedReport {
            spawn_points,
            extra: request.seeds.len(),
            used: stats.seeds_used,
            off_mesh: stats.seeds_off_mesh.iter().map(|v| v.to_array()).collect(),
        },
        props: prop_report,
        threads: rayon::current_num_threads(),
        elapsed_ms: started.elapsed().as_millis(),
        next_steps: vec![
            format!("navinfo {}", output.display()),
            "in game: nav_analyze, to add hiding spots, encounter paths, and visibility".to_string(),
        ],
    })
}

/// The sources prop models are looked for in: the map's own pakfile, then
/// each game directory's loose files and VPKs. Also says what each source is.
fn game_content(
    bsp_bytes: &[u8],
    bsp_path: &Path,
    game_dirs: &[PathBuf],
) -> Result<(SearchPath, Vec<String>), Failure> {
    let mut search = SearchPath::default();
    let mut described = Vec::new();
    let packed = bsp::pakfile_files(bsp_bytes, |path| path.ends_with(".mdl") || path.ends_with(".phy"))
        .with_context(|| format!("reading {}'s pakfile", bsp_path.display()))
        .map_err(Failure::input)?;
    let pakfile = Pakfile::new(packed);
    described.push(format!("pakfile ({} model files)", pakfile.len()));
    search.push(Box::new(pakfile));

    let dirs = if game_dirs.is_empty() { detected_game_dirs(bsp_path) } else { game_dirs.to_vec() };
    for dir in dirs {
        let sources = mount_game_dir(&dir).map_err(Failure::input)?;
        described.push(format!("{} (loose files and {} VPKs)", dir.display(), sources.len() - 1));
        sources.into_iter().for_each(|s| search.push(s));
    }
    Ok((search, described))
}

/// For a map at `<game>/maps/<map>.bsp`: `<game>`, and an `hl2` beside it,
/// which the Source games built on Half-Life 2 mount for shared models.
fn detected_game_dirs(bsp_path: &Path) -> Vec<PathBuf> {
    let Some(maps) = bsp_path.parent().filter(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("maps"))) else {
        return Vec::new();
    };
    let Some(game) = maps.parent() else {
        return Vec::new();
    };
    let mut dirs = vec![game.to_path_buf()];
    let hl2 = game.with_file_name("hl2");
    if hl2.is_dir() && hl2 != game {
        dirs.push(hl2);
    }
    dirs
}
