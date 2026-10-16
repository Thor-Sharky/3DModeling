use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

/// Axis-aligned bounding box. An empty box has `min > max`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) };

    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_center_size(center: Vec3, size: Vec3) -> Self {
        Self { min: center - size * 0.5, max: center + size * 0.5 }
    }

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        let mut b = Self::EMPTY;
        for p in points {
            b.extend(p);
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn extend(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        if self.is_empty() { Vec3::ZERO } else { self.max - self.min }
    }

    /// Radius of the bounding sphere around the center.
    pub fn radius(&self) -> f32 {
        self.size().length() * 0.5
    }

    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }

    /// Bounds of this box after an affine transform.
    pub fn transformed(&self, m: &Mat4) -> Aabb {
        if self.is_empty() {
            return *self;
        }
        Aabb::from_points(self.corners().into_iter().map(|c| m.transform_point3(c)))
    }

    pub fn expanded(&self, by: f32) -> Aabb {
        Aabb { min: self.min - Vec3::splat(by), max: self.max + Vec3::splat(by) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_extend() {
        let mut b = Aabb::EMPTY;
        assert!(b.is_empty());
        assert_eq!(b.size(), Vec3::ZERO);
        b.extend(Vec3::new(1.0, 2.0, 3.0));
        b.extend(Vec3::new(-1.0, 0.0, 5.0));
        assert_eq!(b.min, Vec3::new(-1.0, 0.0, 3.0));
        assert_eq!(b.max, Vec3::new(1.0, 2.0, 5.0));
        assert_eq!(b.center(), Vec3::new(0.0, 1.0, 4.0));
    }

    #[test]
    fn transformed_rotated_box_grows() {
        let b = Aabb::from_center_size(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let m = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4);
        let t = b.transformed(&m);
        assert!((t.size().x - 2.0 * 2f32.sqrt()).abs() < 1e-5);
        assert!((t.size().y - 2.0).abs() < 1e-5);
    }
}
