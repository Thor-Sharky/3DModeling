use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// Position and rotation of an object. Size is never stored as a scale:
/// sizing changes the shape itself (see the "size is locked in" rule), so the
/// transform is always rigid and normals never need special handling.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform { translation: Vec3::ZERO, rotation: Quat::IDENTITY };

    pub fn from_translation(t: Vec3) -> Self {
        Self { translation: t, rotation: Quat::IDENTITY }
    }

    pub fn new(translation: Vec3, rotation: Quat) -> Self {
        Self { translation, rotation }
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_rotation_translation(self.rotation, self.translation)
    }

    pub fn inverse(&self) -> Transform {
        let inv_rot = self.rotation.inverse();
        Transform { translation: inv_rot * -self.translation, rotation: inv_rot }
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        self.rotation * p + self.translation
    }

    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        self.rotation * v
    }

    /// `self * child`: apply `child` first, then `self`.
    pub fn mul(&self, child: &Transform) -> Transform {
        Transform {
            translation: self.transform_point(child.translation),
            rotation: (self.rotation * child.rotation).normalize(),
        }
    }

    /// Local axes (X, Y, Z) in parent space.
    pub fn axes(&self) -> [Vec3; 3] {
        [self.rotation * Vec3::X, self.rotation * Vec3::Y, self.rotation * Vec3::Z]
    }

    pub fn approx_eq(&self, other: &Transform, eps: f32) -> bool {
        (self.translation - other.translation).length() <= eps && self.rotation.dot(other.rotation).abs() >= 1.0 - eps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_roundtrip() {
        let t = Transform::new(Vec3::new(1.0, 2.0, 3.0), Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.3));
        let p = Vec3::new(-4.0, 0.5, 9.0);
        let back = t.inverse().transform_point(t.transform_point(p));
        assert!((back - p).length() < 1e-5);
        assert!(t.mul(&t.inverse()).approx_eq(&Transform::IDENTITY, 1e-5));
    }

    #[test]
    fn matrix_matches_point_transform() {
        let t = Transform::new(Vec3::new(1.0, -2.0, 0.5), Quat::from_rotation_z(1.1));
        let p = Vec3::new(0.3, 0.2, -0.9);
        assert!((t.matrix().transform_point3(p) - t.transform_point(p)).length() < 1e-5);
    }
}
