//! Geometry in Source world units (x east, y north-is-negative, z up).
//! Vectors are `glam`'s; planes and boxes are this crate's own value types.

pub use glam::{Mat3, Vec3};

/// Valve's `AngleMatrix`: a `QAngle` in degrees (`[pitch, yaw, roll]`) as
/// a rotation whose columns are the forward, left, and up axes, so a point
/// in model space maps to the world as `rotation * point + origin`.
pub fn angle_matrix(angles: [f32; 3]) -> Mat3 {
    let [pitch, yaw, roll] = angles.map(f32::to_radians);
    let ((sp, cp), (sy, cy), (sr, cr)) = (pitch.sin_cos(), yaw.sin_cos(), roll.sin_cos());
    let (crcy, crsy, srcy, srsy) = (cr * cy, cr * sy, sr * cy, sr * sy);
    Mat3::from_cols(
        Vec3::new(cp * cy, cp * sy, -sp),
        Vec3::new(sp * srcy - crsy, sp * srsy + crcy, sr * cp),
        Vec3::new(sp * crcy + srsy, sp * crsy - srcy, cr * cp),
    )
}

/// The set of points `p` with `normal · p == dist`. A convex solid lies on the
/// side where `normal · p < dist`, matching the BSP's outward-facing brush sides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub dist: f32,
}

impl Plane {
    pub fn signed_distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) - self.dist
    }

    pub fn translated(&self, by: Vec3) -> Plane {
        Plane { normal: self.normal, dist: self.dist + self.normal.dot(by) }
    }
}

/// An axis-aligned box whose `min` is component-wise no greater than `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    /// The box spanning two opposite corners given in any order.
    pub fn from_corners(a: Vec3, b: Vec3) -> Self {
        Self { min: a.min(b), max: a.max(b) }
    }

    pub fn from_center(center: Vec3, half: Vec3) -> Self {
        Self::from_corners(center - half.abs(), center + half.abs())
    }

    pub fn min(&self) -> Vec3 {
        self.min
    }

    pub fn max(&self) -> Vec3 {
        self.max
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn half_extents(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }

    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    pub fn expanded(&self, by: Vec3) -> Aabb {
        Aabb::from_corners(self.min - by.abs(), self.max + by.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn no_rotation_is_the_identity() {
        assert!(close(angle_matrix([0.0, 0.0, 0.0]) * Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 2.0, 3.0)));
    }

    #[test]
    fn yaw_turns_forward_toward_left() {
        assert!(close(angle_matrix([0.0, 90.0, 0.0]) * Vec3::X, Vec3::Y));
    }

    #[test]
    fn positive_pitch_looks_down() {
        assert!(close(angle_matrix([90.0, 0.0, 0.0]) * Vec3::X, -Vec3::Z));
    }

    #[test]
    fn roll_turns_left_toward_up() {
        assert!(close(angle_matrix([0.0, 0.0, 90.0]) * Vec3::Y, Vec3::Z));
    }

    #[test]
    fn corners_in_any_order_make_the_same_box() {
        let a = Aabb::from_corners(Vec3::new(5.0, -1.0, 3.0), Vec3::new(-5.0, 1.0, -3.0));
        assert_eq!(a.min(), Vec3::new(-5.0, -1.0, -3.0));
        assert_eq!(a.max(), Vec3::new(5.0, 1.0, 3.0));
    }

    #[test]
    fn translating_a_plane_moves_its_distance_along_the_normal() {
        let floor_top = Plane { normal: Vec3::Z, dist: 10.0 };
        let moved = floor_top.translated(Vec3::new(100.0, 0.0, 5.0));
        assert_eq!(moved.dist, 15.0);
    }
}
