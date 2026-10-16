use glam::{Mat4, Vec3};

use crate::{Aabb, Plane};

/// A ray with a (not necessarily normalized) direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

/// Result of a ray/triangle test: distance along the ray and barycentric `u`, `v`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriangleHit {
    pub t: f32,
    pub u: f32,
    pub v: f32,
}

impl Ray {
    pub fn new(origin: Vec3, dir: Vec3) -> Self {
        Self { origin, dir }
    }

    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }

    pub fn normalized(&self) -> Ray {
        Ray { origin: self.origin, dir: self.dir.normalize_or_zero() }
    }

    /// The same ray expressed in another space (e.g. world -> object with the inverse model matrix).
    pub fn transformed(&self, m: &Mat4) -> Ray {
        Ray { origin: m.transform_point3(self.origin), dir: m.transform_vector3(self.dir) }
    }

    /// Möller–Trumbore, two-sided. Returns hits with `t >= 0`.
    pub fn intersect_triangle(&self, a: Vec3, b: Vec3, c: Vec3) -> Option<TriangleHit> {
        let e1 = b - a;
        let e2 = c - a;
        let p = self.dir.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = self.dir.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = e2.dot(q) * inv;
        (t >= 0.0).then_some(TriangleHit { t, u, v })
    }

    /// Slab test. Returns the entry distance (0 if the origin is inside).
    pub fn intersect_aabb(&self, b: &Aabb) -> Option<f32> {
        if b.is_empty() {
            return None;
        }
        let inv = self.dir.recip();
        let t1 = (b.min - self.origin) * inv;
        let t2 = (b.max - self.origin) * inv;
        let tmin = t1.min(t2);
        let tmax = t1.max(t2);
        // NaN-safe max/min across axes.
        let enter = tmin.x.max(tmin.y).max(tmin.z).max(0.0);
        let exit = tmax.x.min(tmax.y).min(tmax.z);
        (enter <= exit).then_some(enter)
    }

    pub fn intersect_plane(&self, plane: &Plane) -> Option<f32> {
        let denom = plane.normal.dot(self.dir);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = -(plane.normal.dot(self.origin) + plane.d) / denom;
        (t >= 0.0).then_some(t)
    }

    /// Closest points between this ray (as an infinite line) and another line.
    /// Returns the parameter on `self` and on `other`; `None` if they are parallel.
    pub fn closest_params_to_line(&self, other: &Ray) -> Option<(f32, f32)> {
        let w0 = self.origin - other.origin;
        let a = self.dir.dot(self.dir);
        let b = self.dir.dot(other.dir);
        let c = other.dir.dot(other.dir);
        let d = self.dir.dot(w0);
        let e = other.dir.dot(w0);
        let denom = a * c - b * b;
        if denom.abs() < 1e-12 {
            return None;
        }
        Some(((b * e - c * d) / denom, (a * e - b * d) / denom))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_hit_and_miss() {
        let r = Ray::new(Vec3::new(0.25, 5.0, 0.25), Vec3::NEG_Y);
        let a = Vec3::ZERO;
        let b = Vec3::new(0.0, 0.0, 1.0);
        let c = Vec3::new(1.0, 0.0, 0.0);
        let hit = r.intersect_triangle(a, b, c).unwrap();
        assert!((hit.t - 5.0).abs() < 1e-6);
        let miss = Ray::new(Vec3::new(2.0, 5.0, 2.0), Vec3::NEG_Y);
        assert!(miss.intersect_triangle(a, b, c).is_none());
        let behind = Ray::new(Vec3::new(0.25, -5.0, 0.25), Vec3::NEG_Y);
        assert!(behind.intersect_triangle(a, b, c).is_none());
    }

    #[test]
    fn aabb_hit_inside_and_axis_aligned() {
        let b = Aabb::from_center_size(Vec3::ZERO, Vec3::splat(2.0));
        let r = Ray::new(Vec3::new(0.0, 0.0, -10.0), Vec3::Z);
        assert!((r.intersect_aabb(&b).unwrap() - 9.0).abs() < 1e-6);
        let inside = Ray::new(Vec3::ZERO, Vec3::X);
        assert_eq!(inside.intersect_aabb(&b), Some(0.0));
        let miss = Ray::new(Vec3::new(0.0, 5.0, -10.0), Vec3::Z);
        assert!(miss.intersect_aabb(&b).is_none());
    }

    #[test]
    fn plane_hit() {
        let p = Plane::from_point_normal(Vec3::ZERO, Vec3::Y);
        let r = Ray::new(Vec3::new(1.0, 3.0, 2.0), Vec3::new(0.0, -1.0, 0.0));
        let t = r.intersect_plane(&p).unwrap();
        assert!((r.at(t) - Vec3::new(1.0, 0.0, 2.0)).length() < 1e-6);
    }

    #[test]
    fn closest_between_lines() {
        let a = Ray::new(Vec3::ZERO, Vec3::X);
        let b = Ray::new(Vec3::new(3.0, 1.0, -5.0), Vec3::Z);
        let (s, t) = a.closest_params_to_line(&b).unwrap();
        assert!((s - 3.0).abs() < 1e-5);
        assert!((t - 5.0).abs() < 1e-5);
    }
}
