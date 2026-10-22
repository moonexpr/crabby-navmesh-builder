//! Prop collision shapes from model files: a `.phy`'s convex ledges as
//! hulls (through `vformats`), and a `.mdl` header's hull box for props
//! that collide as a bounding box.

use crate::core::geom::{Aabb, Vec3};
use crate::core::solid::{Hull, SolidError};

/// `studiohdr_t`'s identifier, "IDST".
const MDL_IDENT: u32 = u32::from_le_bytes(*b"IDST");
/// `studiohdr_t::hull_min` follows id, version, checksum, name\[64\],
/// length, eyeposition, and illumposition.
const MDL_HULL_OFFSET: usize = 4 + 4 + 4 + 64 + 4 + 12 + 12;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ModelError {
    #[error("phy: {0}")]
    Phy(String),
    #[error("phy holds no convex ledges")]
    NoLedges,
    #[error("phy ledge {ledge}: {source}")]
    Ledge { ledge: usize, source: SolidError },
    #[error("mdl is {0} bytes, too short for a studio header")]
    MdlTooShort(usize),
    #[error("not a studio model (ident {0:#010x}, expected \"IDST\")")]
    MdlIdent(u32),
}

/// Every ledge of every solid in a `.phy`, as hulls in model space. A
/// ledge that encloses no volume has nothing to collide with and is left out.
pub fn phy_hulls(bytes: &[u8]) -> Result<Vec<Hull>, ModelError> {
    let phy =
        vformats::phy::parse_lossy(bytes, &vformats::Limits::default()).map_err(|e| ModelError::Phy(e.to_string()))?;
    let mut hulls = Vec::new();
    for (ledge, convex) in phy.ledges().enumerate() {
        let points = convex.vertices.iter().map(|v| Vec3::from_array(*v)).collect();
        match Hull::new(points, convex.triangles.clone()) {
            Ok(hull) => hulls.push(hull),
            Err(SolidError::DegenerateHull) => {}
            Err(source) => return Err(ModelError::Ledge { ledge, source }),
        }
    }
    if hulls.is_empty() {
        return Err(ModelError::NoLedges);
    }
    Ok(hulls)
}

/// The `.mdl` header's hull box, which `SOLID_BBOX` props collide as.
pub fn mdl_hull_box(bytes: &[u8]) -> Result<Hull, ModelError> {
    let field = |at: usize| bytes.get(at..at + 4).map(|b| <[u8; 4]>::try_from(b).expect("four bytes"));
    let (Some(ident), Some(_)) = (field(0), field(MDL_HULL_OFFSET + 20)) else {
        return Err(ModelError::MdlTooShort(bytes.len()));
    };
    let ident = u32::from_le_bytes(ident);
    if ident != MDL_IDENT {
        return Err(ModelError::MdlIdent(ident));
    }
    let float = |i: usize| f32::from_le_bytes(field(MDL_HULL_OFFSET + i * 4).expect("checked the header's length"));
    let min = Vec3::new(float(0), float(1), float(2));
    let max = Vec3::new(float(3), float(4), float(5));
    Ok(Hull::axis_box(Aabb::from_corners(min, max)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A studio header with this hull box and nothing after it.
    pub(crate) fn mdl_with_hull(min: Vec3, max: Vec3) -> Vec<u8> {
        let mut bytes = vec![0u8; MDL_HULL_OFFSET + 24 + 64];
        bytes[0..4].copy_from_slice(&MDL_IDENT.to_le_bytes());
        bytes[4..8].copy_from_slice(&48i32.to_le_bytes());
        for (i, v) in [min.x, min.y, min.z, max.x, max.y, max.z].iter().enumerate() {
            let at = MDL_HULL_OFFSET + i * 4;
            bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn the_mdl_hull_box_comes_from_the_header() {
        let hull =
            mdl_hull_box(&mdl_with_hull(Vec3::new(-10.0, -20.0, 0.0), Vec3::new(10.0, 20.0, 40.0))).expect("valid");
        let xs: Vec<f32> = hull.points().iter().map(|p| p.x).collect();
        assert_eq!(xs.iter().copied().fold(f32::MAX, f32::min), -10.0);
        assert_eq!(hull.points().iter().map(|p| p.z).fold(f32::MIN, f32::max), 40.0);
    }

    #[test]
    fn a_file_that_is_not_a_studio_model_is_refused() {
        assert_eq!(mdl_hull_box(&[0; 10]), Err(ModelError::MdlTooShort(10)));
        let mut not_mdl = mdl_with_hull(Vec3::ZERO, Vec3::ONE);
        not_mdl[0] = b'X';
        assert!(matches!(mdl_hull_box(&not_mdl), Err(ModelError::MdlIdent(_))));
    }

    #[test]
    fn a_file_that_is_not_a_phy_is_refused() {
        assert!(phy_hulls(b"not a physics model at all").is_err());
        assert!(phy_hulls(&[]).is_err());
    }
}
