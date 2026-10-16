//! Snapping helpers. Step values of 0 or less mean "no snapping".

use glam::Vec3;

pub fn snap(v: f32, step: f32) -> f32 {
    if step <= 0.0 { v } else { (v / step).round() * step }
}

pub fn snap_vec3(v: Vec3, step: f32) -> Vec3 {
    Vec3::new(snap(v.x, step), snap(v.y, step), snap(v.z, step))
}

/// Snap an angle in radians to a step given in degrees.
pub fn snap_angle_rad(angle: f32, step_deg: f32) -> f32 {
    if step_deg <= 0.0 { angle } else { snap(angle, step_deg.to_radians()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping() {
        assert_eq!(snap(2.6, 1.0), 3.0);
        assert_eq!(snap(2.6, 0.0), 2.6);
        assert!((snap(0.37, 0.25) - 0.25).abs() < 1e-6);
        assert!((snap(-1.4, 0.5) + 1.5).abs() < 1e-6);
        assert!((snap_angle_rad(50f32.to_radians(), 15.0) - 45f32.to_radians()).abs() < 1e-6);
    }
}
