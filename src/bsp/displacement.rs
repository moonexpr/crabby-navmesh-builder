//! Displacement surfaces, rebuilt the way `CCoreDispInfo::GenerateDispSurf`
//! does: the base quad is rotated to start at the corner nearest
//! `startPosition`, then vertex `(i, j)` (index `i * n + j`) sits at
//! `lerp(lerp(p0, p1, i), lerp(p3, p2, i), j)` plus its offset.

use crate::geom::Vec3;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DisplacementError {
    #[error("power {0} is outside 1 to 4")]
    BadPower(i32),
    #[error("power {power} needs {expected} vertices, found {found}")]
    VertexCount { power: u32, expected: usize, found: usize },
    #[error("base face has {0} edges, not 4")]
    NotAQuad(usize),
}

/// A displacement with its base quad already in generation order.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplacementSurface {
    corners: [Vec3; 4],
    side: usize,
    offsets: Vec<Vec3>,
}

impl DisplacementSurface {
    /// `face_corners` in face winding order; `offsets` are each vertex's
    /// direction times distance, in the lump's order.
    pub fn new(
        face_corners: [Vec3; 4],
        start: Vec3,
        power: i32,
        offsets: Vec<Vec3>,
    ) -> Result<Self, DisplacementError> {
        if !(1..=4).contains(&power) {
            return Err(DisplacementError::BadPower(power));
        }
        let side = (1usize << power) + 1;
        if offsets.len() != side * side {
            return Err(DisplacementError::VertexCount {
                power: power as u32,
                expected: side * side,
                found: offsets.len(),
            });
        }
        let first = (0..4)
            .min_by(|a, b| {
                face_corners[*a].distance_squared(start).total_cmp(&face_corners[*b].distance_squared(start))
            })
            .expect("four corners");
        let corners = std::array::from_fn(|k| face_corners[(k + first) % 4]);
        Ok(Self { corners, side, offsets })
    }

    /// Vertices per side: `2^power + 1`.
    pub fn side(&self) -> usize {
        self.side
    }

    pub fn corners(&self) -> [Vec3; 4] {
        self.corners
    }

    pub fn vertices(&self) -> Vec<Vec3> {
        let [p0, p1, p2, p3] = self.corners;
        let last = (self.side - 1) as f32;
        let mut out = Vec::with_capacity(self.side * self.side);
        for i in 0..self.side {
            let row = i as f32 / last;
            let (from, to) = (p0.lerp(p1, row), p3.lerp(p2, row));
            for j in 0..self.side {
                out.push(from.lerp(to, j as f32 / last) + self.offsets[i * self.side + j]);
            }
        }
        out
    }

    /// Two triangles per grid cell, with alternating diagonals as the engine
    /// tessellates them.
    pub fn triangles(&self) -> Vec<[Vec3; 3]> {
        let v = self.vertices();
        let n = self.side;
        let mut out = Vec::with_capacity((n - 1) * (n - 1) * 2);
        for i in 0..n - 1 {
            for j in 0..n - 1 {
                let (a, b, c, d) = (v[i * n + j], v[i * n + j + 1], v[(i + 1) * n + j], v[(i + 1) * n + j + 1]);
                if (i * (n - 1) + j).is_multiple_of(2) {
                    out.push([a, c, d]);
                    out.push([a, d, b]);
                } else {
                    out.push([a, c, b]);
                    out.push([c, d, b]);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> [Vec3; 4] {
        [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 100.0, 0.0), Vec3::new(100.0, 100.0, 0.0), Vec3::new(100.0, 0.0, 0.0)]
    }

    #[test]
    fn the_base_quad_starts_at_the_corner_nearest_start() {
        let surface =
            DisplacementSurface::new(square(), Vec3::new(99.0, 99.0, 0.0), 1, vec![Vec3::ZERO; 9]).expect("valid");
        assert_eq!(surface.corners()[0], Vec3::new(100.0, 100.0, 0.0));
        assert_eq!(surface.corners()[1], Vec3::new(100.0, 0.0, 0.0));
    }

    #[test]
    fn rows_run_from_p0_toward_p1_and_columns_toward_p3() {
        let surface = DisplacementSurface::new(square(), Vec3::ZERO, 1, vec![Vec3::ZERO; 9]).expect("valid");
        let v = surface.vertices();
        assert_eq!(surface.side(), 3);
        assert_eq!(v[1], Vec3::new(50.0, 0.0, 0.0)); // i = 0, j = 1: toward p3
        assert_eq!(v[3], Vec3::new(0.0, 50.0, 0.0)); // i = 1, j = 0: toward p1
        assert_eq!(v[8], Vec3::new(100.0, 100.0, 0.0));
    }

    #[test]
    fn offsets_displace_each_vertex() {
        let mut offsets = vec![Vec3::ZERO; 9];
        offsets[4] = Vec3::new(0.0, 0.0, 32.0);
        let surface = DisplacementSurface::new(square(), Vec3::ZERO, 1, offsets).expect("valid");
        assert_eq!(surface.vertices()[4], Vec3::new(50.0, 50.0, 32.0));
    }

    #[test]
    fn each_cell_becomes_two_triangles() {
        let surface = DisplacementSurface::new(square(), Vec3::ZERO, 2, vec![Vec3::ZERO; 25]).expect("valid");
        assert_eq!(surface.triangles().len(), 4 * 4 * 2);
    }

    #[test]
    fn the_vertex_count_must_match_the_power() {
        assert_eq!(
            DisplacementSurface::new(square(), Vec3::ZERO, 2, vec![Vec3::ZERO; 9]),
            Err(DisplacementError::VertexCount { power: 2, expected: 25, found: 9 })
        );
        assert_eq!(DisplacementSurface::new(square(), Vec3::ZERO, 7, vec![]), Err(DisplacementError::BadPower(7)));
    }
}
