//! Turns decoded lumps into a [`Map`]: walks every model's BSP tree for the
//! brushes that block players, and rebuilds displacements as triangle
//! prisms. Which brush entities stay solid is a game's decision, made later
//! by `core::world`; this only reports what the map holds.

use std::collections::BTreeSet;

use crate::core::geom::{Plane, Vec3};
use crate::core::solid::{ConvexSolid, SolidError};
use crate::core::world::{MapContents, StaticProp};

use super::displacement::{DisplacementError, DisplacementSurface};
use super::entities::parse_entities;
use super::lumps::{LumpId, Lumps};
use super::records::{
    RawBrush, RawBrushSide, RawDispInfo, RawDispVert, RawEdge, RawFace, RawLeaf, RawLeafBrush, RawModel, RawNode,
    RawSurfEdge, Record, records,
};
use super::{BspError, Contents, Map, MapStats};

/// Displacement prisms reach this far behind their surface.
pub const DISPLACEMENT_THICKNESS: f32 = 4.0;

struct Tables {
    planes: Vec<Plane>,
    brushes: Vec<RawBrush>,
    sides: Vec<RawBrushSide>,
    nodes: Vec<RawNode>,
    leafs: Vec<RawLeaf>,
    leaf_brushes: Vec<RawLeafBrush>,
    models: Vec<RawModel>,
}

fn table<R: Record>(lumps: &Lumps<'_>) -> Result<Vec<R>, BspError> {
    records(&lumps.get(R::LUMP)?, lumps.version())
}

fn get<'a, T>(items: &'a [T], index: i64, what: &'static str) -> Result<&'a T, BspError> {
    usize::try_from(index).ok().and_then(|i| items.get(i)).ok_or(BspError::BadIndex { what, index })
}

pub fn assemble(lumps: &Lumps<'_>, props: Vec<StaticProp>) -> Result<Map, BspError> {
    let tables = Tables {
        planes: table(lumps)?,
        brushes: table(lumps)?,
        sides: table(lumps)?,
        nodes: table(lumps)?,
        leafs: table(lumps)?,
        leaf_brushes: table(lumps)?,
        models: table(lumps)?,
    };
    let text = lumps.get(LumpId::Entities)?;
    let entities = parse_entities(&String::from_utf8_lossy(&text))?;
    let mut stats = MapStats::default();

    let mut models = Vec::with_capacity(tables.models.len());
    for model in 0..tables.models.len() {
        let mut solids = Vec::new();
        for brush in tables.model_brushes(model)? {
            if let Some(solid) = tables.brush_solid(brush)? {
                solids.push(solid);
            }
        }
        if model == 0 {
            stats.brushes = solids.len();
        }
        models.push(solids);
    }
    stats.brush_models = models.len().saturating_sub(1);

    let prisms = displacement_prisms(lumps, &tables.planes, &mut stats)?;
    stats.static_props = props.len();
    let contents = MapContents::new(entities, models, prisms, props)?;
    Ok(Map { version: lumps.version(), contents, stats })
}

impl Tables {
    /// Every brush in the leaves under a model's head node, each once.
    fn model_brushes(&self, model: usize) -> Result<BTreeSet<usize>, BspError> {
        let head = get(&self.models, model as i64, "model")?.head_node;
        let mut brushes = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut stack = vec![head];
        while let Some(child) = stack.pop() {
            if child >= 0 {
                if visited.insert(child) {
                    stack.extend(get(&self.nodes, child as i64, "node")?.children);
                }
                continue;
            }
            let leaf = get(&self.leafs, -1 - child as i64, "leaf")?;
            let first = leaf.first_leaf_brush as i64;
            for slot in first..first + leaf.leaf_brush_count as i64 {
                brushes.insert(get(&self.leaf_brushes, slot, "leaf brush")?.0 as usize);
            }
        }
        Ok(brushes)
    }

    /// The brush as a solid, or `None` when players pass through it.
    fn brush_solid(&self, index: usize) -> Result<Option<ConvexSolid>, BspError> {
        let brush = get(&self.brushes, index as i64, "brush")?;
        if !Contents::from_bits(brush.contents).intersects(Contents::PLAYER_SOLID) {
            return Ok(None);
        }
        let first = brush.first_side as i64;
        let planes = (first..first + brush.side_count as i64)
            .map(|s| {
                let side = get(&self.sides, s, "brush side")?;
                get(&self.planes, side.plane as i64, "plane").copied()
            })
            .collect::<Result<Vec<_>, _>>()?;
        ConvexSolid::new(planes).map(Some).map_err(|source| BspError::Brush { brush: index, source })
    }
}

fn displacement_prisms(
    lumps: &Lumps<'_>,
    planes: &[Plane],
    stats: &mut MapStats,
) -> Result<Vec<ConvexSolid>, BspError> {
    let infos: Vec<RawDispInfo> = table(lumps)?;
    if infos.is_empty() {
        return Ok(Vec::new());
    }
    let faces: Vec<RawFace> = table(lumps)?;
    let verts: Vec<RawDispVert> = table(lumps)?;
    let edges: Vec<RawEdge> = table(lumps)?;
    let surf_edges: Vec<RawSurfEdge> = table(lumps)?;
    let vertexes: Vec<Vec3> = table(lumps)?;

    let mut prisms = Vec::new();
    for (index, info) in infos.iter().enumerate() {
        let fail = |source: DisplacementError| BspError::Displacement { index, source };
        let face = get(&faces, info.map_face as i64, "face")?;
        if face.edge_count != 4 {
            return Err(fail(DisplacementError::NotAQuad(face.edge_count.max(0) as usize)));
        }
        let mut corners = [Vec3::ZERO; 4];
        for (k, corner) in corners.iter_mut().enumerate() {
            let surf_edge = get(&surf_edges, face.first_edge as i64 + k as i64, "surface edge")?.0;
            let edge = get(&edges, (surf_edge as i64).abs(), "edge")?;
            let vertex = if surf_edge >= 0 { edge.0[0] } else { edge.0[1] };
            *corner = *get(&vertexes, vertex as i64, "vertex")?;
        }
        let count = if (1..=4).contains(&info.power) { ((1i64 << info.power) + 1).pow(2) } else { 0 };
        let first = info.first_vert as i64;
        let offsets = (first..first + count)
            .map(|v| get(&verts, v, "displacement vertex").map(|d| d.direction * d.distance))
            .collect::<Result<Vec<_>, _>>()?;
        let surface = DisplacementSurface::new(corners, info.start, info.power, offsets).map_err(fail)?;

        let plane = get(planes, face.plane as i64, "plane")?;
        let up = if face.side == 0 { plane.normal } else { -plane.normal };
        for triangle in surface.triangles() {
            match ConvexSolid::from_triangle(triangle, up, DISPLACEMENT_THICKNESS) {
                Ok(prism) => prisms.push(prism),
                Err(SolidError::DegenerateTriangle) => {} // collapsed vertices have no surface to collide with
                Err(other) => unreachable!("prisms always carry axial planes: {other}"),
            }
        }
        stats.displacements += 1;
    }
    stats.displacement_triangles = prisms.len();
    Ok(prisms)
}
