//! What `navinfo` reports about a mesh: counts, sizes, connectivity, and
//! the problems an agent or mapper would act on.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::core::mesh::{AreaId, Attributes, Direction, HidingFlags, NavArea, NavMesh, Visibility};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NavAnalysis {
    pub areas: AreaSummary,
    pub connections: ConnectionSummary,
    pub components: ComponentSummary,
    pub tactics: TacticsSummary,
    /// Findings worth acting on, in plain words; empty when none.
    pub findings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaSummary {
    pub count: usize,
    pub crouch: usize,
    pub jump: usize,
    /// Total footprint in square units.
    pub floor_area: f64,
    /// Footprints of the smallest, median, and largest areas; absent when empty.
    pub smallest: Option<f32>,
    pub median: Option<f32>,
    pub largest: Option<f32>,
    pub bounds_min: Option<[f32; 3]>,
    pub bounds_max: Option<[f32; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ConnectionSummary {
    pub total: usize,
    /// Pairs connected in both directions, counted once.
    pub two_way_pairs: usize,
    /// Connections whose target does not connect back (drops, for example).
    pub one_way: usize,
    /// Connections to ids no area has.
    pub dangling: usize,
}

/// What analysis (`nav_analyze`, or `navgen2`) added.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TacticsSummary {
    pub hiding_spots: usize,
    pub in_cover: usize,
    pub sniper_spots: usize,
    pub areas_with_visibility: usize,
    pub visible_area_entries: usize,
    pub completely_visible_entries: usize,
    /// Areas whose visible set overrides another area's.
    pub inheriting_areas: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComponentSummary {
    /// Groups of areas joined by connections in either direction.
    pub count: usize,
    pub largest: usize,
    /// Areas with no connections in or out.
    pub isolated: usize,
}

/// One area as a flat table row, for `navinfo --areas`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaRow {
    pub id: u32,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub z_nw: f32,
    pub z_ne: f32,
    pub z_se: f32,
    pub z_sw: f32,
    pub crouch: bool,
    pub north: usize,
    pub east: usize,
    pub south: usize,
    pub west: usize,
}

/// One area in full, for `navinfo --area ID`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AreaDetail {
    pub id: u32,
    pub attributes: u32,
    pub north_west: [f32; 3],
    pub south_east: [f32; 3],
    pub z_north_east: f32,
    pub z_south_west: f32,
    pub north: Vec<u32>,
    pub east: Vec<u32>,
    pub south: Vec<u32>,
    pub west: Vec<u32>,
}

pub fn analyze(file: &NavMesh) -> NavAnalysis {
    let areas = &file.areas;
    let index: HashMap<AreaId, usize> = areas.iter().enumerate().map(|(i, a)| (a.id(), i)).collect();
    let targets = |a: &NavArea| -> Vec<AreaId> {
        Direction::ALL.iter().flat_map(|d| a.connections(*d).iter().copied()).collect()
    };

    let mut connections = ConnectionSummary { total: 0, two_way_pairs: 0, one_way: 0, dangling: 0 };
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); areas.len()];
    let outgoing: Vec<HashSet<AreaId>> = areas.iter().map(|a| targets(a).into_iter().collect()).collect();
    for (from, area) in areas.iter().enumerate() {
        for to in targets(area) {
            connections.total += 1;
            let Some(&to_index) = index.get(&to) else {
                connections.dangling += 1;
                continue;
            };
            neighbors[from].push(to_index);
            neighbors[to_index].push(from);
            if !outgoing[to_index].contains(&area.id()) {
                connections.one_way += 1;
            } else if area.id() < to {
                connections.two_way_pairs += 1;
            }
        }
    }

    let components = components(&neighbors);
    let mut footprints: Vec<f32> = areas.iter().map(footprint).collect();
    footprints.sort_by(f32::total_cmp);
    let corners = areas.iter().flat_map(|a| {
        let h = a.heights();
        let (nw, se) = (a.north_west(), a.south_east());
        [
            [nw.x, nw.y, h.north_west.min(h.north_east).min(h.south_east).min(h.south_west)],
            [se.x, se.y, h.north_west.max(h.north_east).max(h.south_east).max(h.south_west)],
        ]
    });
    let (bounds_min, bounds_max) = corners.fold((None, None), |(lo, hi): (Option<[f32; 3]>, Option<[f32; 3]>), p| {
        let lo = Some(lo.map_or(p, |l| [l[0].min(p[0]), l[1].min(p[1]), l[2].min(p[2])]));
        let hi = Some(hi.map_or(p, |h| [h[0].max(p[0]), h[1].max(p[1]), h[2].max(p[2])]));
        (lo, hi)
    });
    let summary = AreaSummary {
        count: areas.len(),
        crouch: areas.iter().filter(|a| a.attributes().contains(Attributes::CROUCH)).count(),
        jump: areas.iter().filter(|a| a.attributes().contains(Attributes::JUMP)).count(),
        floor_area: footprints.iter().map(|f| f64::from(*f)).sum(),
        smallest: footprints.first().copied(),
        median: footprints.get(footprints.len() / 2).copied(),
        largest: footprints.last().copied(),
        bounds_min,
        bounds_max,
    };

    let spots = || areas.iter().flat_map(|a| a.hiding_spots().iter());
    let entries = || areas.iter().flat_map(|a| a.visible_areas().iter());
    let tactics = TacticsSummary {
        hiding_spots: spots().count(),
        in_cover: spots().filter(|s| s.flags.contains(HidingFlags::IN_COVER)).count(),
        sniper_spots: spots()
            .filter(|s| {
                s.flags.contains(HidingFlags::GOOD_SNIPER_SPOT) || s.flags.contains(HidingFlags::IDEAL_SNIPER_SPOT)
            })
            .count(),
        areas_with_visibility: areas.iter().filter(|a| !a.visible_areas().is_empty()).count(),
        visible_area_entries: entries().count(),
        completely_visible_entries: entries()
            .filter(|v| v.visibility.bits() & Visibility::COMPLETELY.bits() != 0)
            .count(),
        inheriting_areas: areas.iter().filter(|a| a.inherit_visibility_from().is_some()).count(),
    };
    let findings = findings(file, &summary, &connections, &components);
    NavAnalysis { areas: summary, connections, components, tactics, findings }
}

fn footprint(area: &NavArea) -> f32 {
    let (nw, se) = (area.north_west(), area.south_east());
    (se.x - nw.x) * (se.y - nw.y)
}

/// Groups of areas joined by connections either way, from undirected adjacency.
fn components(neighbors: &[Vec<usize>]) -> ComponentSummary {
    let mut component = vec![usize::MAX; neighbors.len()];
    let mut sizes = Vec::new();
    for start in 0..neighbors.len() {
        if component[start] != usize::MAX {
            continue;
        }
        let label = sizes.len();
        let mut size = 0;
        let mut stack = vec![start];
        component[start] = label;
        while let Some(area) = stack.pop() {
            size += 1;
            for next in &neighbors[area] {
                if component[*next] == usize::MAX {
                    component[*next] = label;
                    stack.push(*next);
                }
            }
        }
        sizes.push(size);
    }
    ComponentSummary {
        count: sizes.len(),
        largest: sizes.iter().copied().max().unwrap_or(0),
        isolated: neighbors.iter().filter(|n| n.is_empty()).count(),
    }
}

fn findings(
    file: &NavMesh,
    areas: &AreaSummary,
    connections: &ConnectionSummary,
    components: &ComponentSummary,
) -> Vec<String> {
    let mut out = Vec::new();
    if areas.count == 0 {
        out.push("the mesh has no areas".to_string());
        return out;
    }
    if !file.analyzed {
        out.push("not analyzed: run nav_analyze in game for hiding spots, encounter paths, and visibility".to_string());
    }
    if connections.dangling > 0 {
        out.push(format!("{} dangling connections point at area ids that do not exist", connections.dangling));
    }
    if components.count > 1 {
        out.push(format!(
            "{} separate components; the largest holds {} of {} areas, so bots cannot path between the rest",
            components.count, components.largest, areas.count
        ));
    }
    if components.isolated > 0 {
        out.push(format!("{} areas have no connections at all", components.isolated));
    }
    out
}

pub fn area_rows(areas: &[NavArea]) -> Vec<AreaRow> {
    areas
        .iter()
        .map(|a| {
            let (nw, se, h) = (a.north_west(), a.south_east(), a.heights());
            let count = |d: Direction| a.connections(d).len();
            AreaRow {
                id: a.id().get(),
                min_x: nw.x,
                min_y: nw.y,
                max_x: se.x,
                max_y: se.y,
                z_nw: h.north_west,
                z_ne: h.north_east,
                z_se: h.south_east,
                z_sw: h.south_west,
                crouch: a.attributes().contains(Attributes::CROUCH),
                north: count(Direction::North),
                east: count(Direction::East),
                south: count(Direction::South),
                west: count(Direction::West),
            }
        })
        .collect()
}

pub fn area_detail(area: &NavArea) -> AreaDetail {
    let ids = |d: Direction| area.connections(d).iter().map(|id| id.get()).collect();
    let heights = area.heights();
    AreaDetail {
        id: area.id().get(),
        attributes: area.attributes().bits(),
        north_west: area.north_west().to_array(),
        south_east: area.south_east().to_array(),
        z_north_east: heights.north_east,
        z_south_west: heights.south_west,
        north: ids(Direction::North),
        east: ids(Direction::East),
        south: ids(Direction::South),
        west: ids(Direction::West),
    }
}

/// The area with this id, if the file has one.
pub fn find_area(areas: &[NavArea], id: AreaId) -> Option<&NavArea> {
    areas.iter().find(|a| a.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mesh::CornerHeights;

    fn id(n: u32) -> AreaId {
        AreaId::new(n).expect("nonzero")
    }

    fn area(n: u32, x: f32, attributes: Attributes, connections: [Vec<AreaId>; 4]) -> NavArea {
        let flat = CornerHeights { north_west: 0.0, north_east: 0.0, south_east: 0.0, south_west: 0.0 };
        NavArea::new(id(n), attributes, (x, 0.0), (x + 50.0, 25.0), flat, connections).expect("valid")
    }

    fn file(areas: Vec<NavArea>) -> NavMesh {
        NavMesh { bsp_size: 10, analyzed: false, areas }
    }

    #[test]
    fn an_empty_mesh_reports_zeroes_and_says_so() {
        let report = analyze(&file(vec![]));
        assert_eq!(report.areas.count, 0);
        assert_eq!(report.areas.median, None);
        assert_eq!(report.components.count, 0);
        assert!(report.findings.iter().any(|f| f.contains("no areas")));
    }

    #[test]
    fn two_way_and_one_way_connections_are_told_apart() {
        let areas = vec![
            area(1, 0.0, Attributes::NONE, [vec![], vec![id(2)], vec![], vec![]]),
            area(2, 50.0, Attributes::NONE, [vec![], vec![id(3)], vec![], vec![id(1)]]),
            area(3, 100.0, Attributes::CROUCH, Default::default()),
        ];
        let report = analyze(&file(areas));
        assert_eq!(report.connections.total, 3);
        assert_eq!(report.connections.two_way_pairs, 1);
        assert_eq!(report.connections.one_way, 1);
        assert_eq!(report.components.count, 1);
        assert_eq!(report.components.largest, 3);
        assert_eq!(report.areas.crouch, 1);
    }

    #[test]
    fn islands_and_dangling_ids_are_findings() {
        let areas = vec![
            area(1, 0.0, Attributes::NONE, [vec![], vec![id(9)], vec![], vec![]]),
            area(2, 500.0, Attributes::NONE, Default::default()),
        ];
        let report = analyze(&file(areas));
        assert_eq!(report.connections.dangling, 1);
        assert_eq!(report.components.count, 2);
        assert_eq!(report.components.isolated, 2, "a dangling link leads nowhere");
        assert!(report.findings.iter().any(|f| f.contains("dangling")));
        assert!(report.findings.iter().any(|f| f.contains("not analyzed")));
    }

    #[test]
    fn footprints_sum_and_rank() {
        let areas = vec![
            area(1, 0.0, Attributes::NONE, Default::default()),
            NavArea::new(
                id(2),
                Attributes::NONE,
                (0.0, 100.0),
                (100.0, 200.0),
                CornerHeights { north_west: 5.0, north_east: 5.0, south_east: 9.0, south_west: 9.0 },
                Default::default(),
            )
            .expect("valid"),
        ];
        let report = analyze(&file(areas));
        assert_eq!(report.areas.floor_area, 1250.0 + 10000.0);
        assert_eq!(report.areas.smallest, Some(1250.0));
        assert_eq!(report.areas.largest, Some(10000.0));
        assert_eq!(report.areas.bounds_min, Some([0.0, 0.0, 0.0]));
        assert_eq!(report.areas.bounds_max, Some([100.0, 200.0, 9.0]));
    }

    #[test]
    fn rows_and_details_flatten_an_area() {
        let a = area(4, 25.0, Attributes::CROUCH, [vec![id(1), id(2)], vec![], vec![id(3)], vec![]]);
        let row = &area_rows(std::slice::from_ref(&a))[0];
        assert_eq!((row.id, row.min_x, row.max_x, row.north, row.south, row.crouch), (4, 25.0, 75.0, 2, 1, true));
        let detail = area_detail(&a);
        assert_eq!(detail.north, vec![1, 2]);
        assert_eq!(detail.attributes, 1);
        assert_eq!(find_area(std::slice::from_ref(&a), id(4)), Some(&a));
    }
}
