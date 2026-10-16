//! Small math layer shared by every Shapeshop crate.
//!
//! Conventions (used everywhere, including exports):
//! - Right-handed, **Y is up**, the ground is the XZ plane.
//! - The "front" of a model faces **-Z** (Roblox `CFrame.LookVector`).
//! - In Roblox projects 1 unit = 1 stud; otherwise 1 unit = 1 meter.

pub mod aabb;
pub mod camera;
pub mod plane;
pub mod ray;
pub mod snap;
pub mod transform;
pub mod units;

pub use aabb::Aabb;
pub use camera::{CameraView, OrbitCamera, Viewport};
pub use glam::{self, DMat4, DQuat, DVec3, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
pub use plane::Plane;
pub use ray::{Ray, TriangleHit};
pub use transform::Transform;
pub use units::Units;

/// Distance below which two points are considered the same point.
pub const EPS: f32 = 1e-5;

/// Closest distance from point `p` to the 2D segment `a`-`b`, plus the segment parameter `t` in 0..=1.
pub fn point_segment_distance_2d(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 <= f32::EPSILON { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
    ((a + ab * t - p).length(), t)
}

/// Newell's method: a robust normal (not normalized) for a possibly non-planar polygon.
pub fn newell_normal(points: impl IntoIterator<Item = Vec3> + Clone) -> Vec3 {
    let pts: Vec<Vec3> = points.into_iter().collect();
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        n.x += (a.y - b.y) * (a.z + b.z);
        n.y += (a.z - b.z) * (a.x + b.x);
        n.z += (a.x - b.x) * (a.y + b.y);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_distance() {
        let (d, t) = point_segment_distance_2d(Vec2::new(1.0, 1.0), Vec2::ZERO, Vec2::new(2.0, 0.0));
        assert!((d - 1.0).abs() < 1e-6);
        assert!((t - 0.5).abs() < 1e-6);
        let (d, t) = point_segment_distance_2d(Vec2::new(-1.0, 0.0), Vec2::ZERO, Vec2::new(2.0, 0.0));
        assert!((d - 1.0).abs() < 1e-6);
        assert_eq!(t, 0.0);
    }

    #[test]
    fn newell_ccw_square_points_up() {
        // CCW seen from +Y.
        let sq =
            [Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)];
        let n = newell_normal(sq).normalize();
        assert!((n - Vec3::Y).length() < 1e-6, "{n}");
    }
}
