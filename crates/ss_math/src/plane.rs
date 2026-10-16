use glam::Vec3;

/// Plane `normal · p + d = 0` with a unit normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub normal: Vec3,
    pub d: f32,
}

impl Plane {
    pub fn from_point_normal(point: Vec3, normal: Vec3) -> Self {
        let n = normal.normalize_or_zero();
        Self { normal: n, d: -n.dot(point) }
    }

    pub fn signed_distance(&self, p: Vec3) -> f32 {
        self.normal.dot(p) + self.d
    }

    pub fn project(&self, p: Vec3) -> Vec3 {
        p - self.normal * self.signed_distance(p)
    }

    /// Any point on the plane.
    pub fn origin(&self) -> Vec3 {
        -self.normal * self.d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_and_projection() {
        let p = Plane::from_point_normal(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 5.0, 0.0));
        assert!((p.signed_distance(Vec3::new(3.0, 5.0, 1.0)) - 3.0).abs() < 1e-6);
        assert!((p.project(Vec3::new(3.0, 5.0, 1.0)) - Vec3::new(3.0, 2.0, 1.0)).length() < 1e-6);
        assert!((p.origin() - Vec3::new(0.0, 2.0, 0.0)).length() < 1e-6);
    }
}
