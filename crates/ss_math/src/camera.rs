//! The viewport camera: orbits around a focus point.
//!
//! All gestures (mouse presets, trackpad, on-screen buttons, scripts) end up as
//! one of the small operations here, so they are unit-testable without a GPU.

use glam::{Mat4, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::{Aabb, Ray};

/// A screen rectangle in logical pixels (origin top-left, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub min: Vec2,
    pub size: Vec2,
}

impl Viewport {
    pub fn new(min: Vec2, size: Vec2) -> Self {
        Self { min, size: size.max(Vec2::splat(1.0)) }
    }

    pub fn aspect(&self) -> f32 {
        self.size.x / self.size.y
    }

    /// Screen point -> normalized device coordinates (-1..1, y up).
    pub fn to_ndc(&self, p: Vec2) -> Vec2 {
        let local = (p - self.min) / self.size;
        Vec2::new(local.x * 2.0 - 1.0, 1.0 - local.y * 2.0)
    }

    pub fn from_ndc(&self, ndc: Vec2) -> Vec2 {
        self.min + Vec2::new((ndc.x + 1.0) * 0.5, (1.0 - ndc.y) * 0.5) * self.size
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.min + self.size).all()
    }
}

/// Named views used by the view cube and menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CameraView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Home,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrbitCamera {
    /// The point the camera orbits around and looks at.
    pub focus: Vec3,
    /// Rotation around the Y axis in radians. 0 = looking from +Z toward -Z... see [`OrbitCamera::eye`].
    pub yaw: f32,
    /// Elevation in radians; positive looks down at the focus.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y_deg: f32,
    /// "Flat view": orthographic projection with the same framing.
    pub ortho: bool,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self::home()
    }
}

pub const MIN_DISTANCE: f32 = 0.2;
pub const MAX_DISTANCE: f32 = 4000.0;
const PITCH_LIMIT: f32 = 89.0 * std::f32::consts::PI / 180.0;

impl OrbitCamera {
    /// The default view: a little above and to the front-right of the origin.
    pub fn home() -> Self {
        Self {
            focus: Vec3::new(0.0, 1.0, 0.0),
            yaw: 35f32.to_radians(),
            pitch: 28f32.to_radians(),
            distance: 18.0,
            fov_y_deg: 50.0,
            ortho: false,
        }
    }

    /// Unit vector from the focus toward the eye.
    pub fn back_dir(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(cp * sy, sp, cp * cy)
    }

    pub fn eye(&self) -> Vec3 {
        self.focus + self.back_dir() * self.distance
    }

    pub fn forward(&self) -> Vec3 {
        -self.back_dir()
    }

    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize_or(Vec3::X)
    }

    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward()).normalize_or(Vec3::Y)
    }

    pub fn near(&self) -> f32 {
        (self.distance * 0.005).clamp(0.01, 1.0)
    }

    pub fn far(&self) -> f32 {
        (self.distance * 50.0).max(1000.0)
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.focus, Vec3::Y)
    }

    /// Half the visible height at the focus distance (used by the flat view).
    pub fn half_height_at_focus(&self) -> f32 {
        self.distance * (self.fov_y_deg.to_radians() * 0.5).tan()
    }

    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        if self.ortho {
            let h = self.half_height_at_focus();
            let w = h * aspect;
            Mat4::orthographic_rh_gl(-w, w, -h, h, -self.far(), self.far())
        } else {
            Mat4::perspective_rh_gl(self.fov_y_deg.to_radians(), aspect, self.near(), self.far())
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection_matrix(aspect) * self.view_matrix()
    }

    /// World-space ray through a screen point.
    pub fn ray_through(&self, vp: &Viewport, screen: Vec2) -> Ray {
        let ndc = vp.to_ndc(screen);
        let inv = self.view_proj(vp.aspect()).inverse();
        let near = inv * Vec4::new(ndc.x, ndc.y, -1.0, 1.0);
        let far = inv * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let near = near.truncate() / near.w;
        let far = far.truncate() / far.w;
        Ray::new(near, (far - near).normalize_or(self.forward()))
    }

    /// Project a world point to the screen. `None` if it is behind the camera.
    pub fn project(&self, vp: &Viewport, p: Vec3) -> Option<Vec2> {
        let clip = self.view_proj(vp.aspect()) * p.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(vp.from_ndc(Vec2::new(ndc.x, ndc.y)))
    }

    /// World units covered by one screen pixel at the focus distance.
    pub fn world_per_pixel(&self, vp: &Viewport) -> f32 {
        2.0 * self.half_height_at_focus() / vp.size.y
    }

    /// World units covered by one screen pixel at a given point.
    pub fn world_per_pixel_at(&self, vp: &Viewport, p: Vec3) -> f32 {
        if self.ortho {
            return self.world_per_pixel(vp);
        }
        let depth = (p - self.eye()).dot(self.forward()).max(self.near());
        2.0 * depth * (self.fov_y_deg.to_radians() * 0.5).tan() / vp.size.y
    }

    /// Rotate around the focus. Positive `dx` turns the view to the right.
    pub fn orbit(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw - d_yaw).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch + d_pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Slide the camera and focus by a screen-space drag in pixels (content follows the cursor).
    pub fn pan_pixels(&mut self, vp: &Viewport, delta: Vec2) {
        let s = self.world_per_pixel(vp);
        self.focus += (-self.right() * delta.x + self.up() * delta.y) * s;
    }

    /// Zoom by a factor (> 1 moves closer). If `toward` is set, the point under the
    /// cursor stays under the cursor, like Roblox Studio and most map apps.
    pub fn zoom(&mut self, factor: f32, toward: Option<Vec3>) {
        if factor <= 0.0 || !factor.is_finite() {
            return;
        }
        let old = self.distance;
        let new = (old / factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
        if let Some(p) = toward {
            let k = 1.0 - new / old;
            self.focus += (p - self.focus) * k;
        }
        self.distance = new;
    }

    /// Fly relative to the view: x = right, y = world up, z = forward.
    pub fn fly(&mut self, local: Vec3) {
        let f = Vec3::new(self.forward().x, 0.0, self.forward().z).normalize_or(Vec3::NEG_Z);
        let delta = self.right() * local.x + Vec3::Y * local.y + f * local.z;
        self.focus += delta;
    }

    /// Fit a box in view, keeping the viewing direction.
    pub fn frame(&mut self, bounds: &Aabb) {
        if bounds.is_empty() {
            return;
        }
        let radius = bounds.radius().max(0.5);
        let half_fov = (self.fov_y_deg.to_radians() * 0.5).max(0.1);
        self.focus = bounds.center();
        self.distance = (radius / half_fov.sin() * 1.15).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    pub fn set_view(&mut self, view: CameraView) {
        use std::f32::consts::{FRAC_PI_2, PI};
        let top = PITCH_LIMIT;
        match view {
            // The model's front faces -Z, so the "Front" view looks from -Z toward +Z.
            CameraView::Front => (self.yaw, self.pitch) = (PI, 0.0),
            CameraView::Back => (self.yaw, self.pitch) = (0.0, 0.0),
            CameraView::Right => (self.yaw, self.pitch) = (FRAC_PI_2, 0.0),
            CameraView::Left => (self.yaw, self.pitch) = (-FRAC_PI_2 + std::f32::consts::TAU, 0.0),
            CameraView::Top => (self.yaw, self.pitch) = (PI, top),
            CameraView::Bottom => (self.yaw, self.pitch) = (PI, -top),
            CameraView::Home => *self = Self { ortho: self.ortho, ..Self::home() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp() -> Viewport {
        Viewport::new(Vec2::new(100.0, 50.0), Vec2::new(800.0, 600.0))
    }

    #[test]
    fn center_ray_hits_focus() {
        let cam = OrbitCamera::home();
        let v = vp();
        let center = v.min + v.size * 0.5;
        let r = cam.ray_through(&v, center);
        let to_focus = (cam.focus - r.origin).normalize();
        assert!(r.dir.dot(to_focus) > 0.9999, "{:?}", r);
    }

    #[test]
    fn project_roundtrip() {
        let cam = OrbitCamera::home();
        let v = vp();
        let p = Vec3::new(1.5, 0.5, -2.0);
        let s = cam.project(&v, p).unwrap();
        let r = cam.ray_through(&v, s);
        let t = (p - r.origin).dot(r.dir);
        assert!((r.at(t) - p).length() < 1e-3);
    }

    #[test]
    fn ortho_rays_are_parallel() {
        let cam = OrbitCamera { ortho: true, ..OrbitCamera::home() };
        let v = vp();
        let a = cam.ray_through(&v, Vec2::new(150.0, 100.0));
        let b = cam.ray_through(&v, Vec2::new(700.0, 500.0));
        assert!(a.dir.dot(b.dir) > 0.99999);
    }

    #[test]
    fn zoom_toward_point_keeps_point_fixed_on_screen() {
        let mut cam = OrbitCamera::home();
        let v = vp();
        let p = Vec3::new(3.0, 0.0, 1.0);
        let before = cam.project(&v, p).unwrap();
        cam.zoom(1.5, Some(p));
        let after = cam.project(&v, p).unwrap();
        assert!((before - after).length() < 0.05, "{before} vs {after}");
        assert!((cam.distance - 12.0).abs() < 1e-4);
    }

    #[test]
    fn pan_moves_content_with_cursor() {
        let mut cam = OrbitCamera::home();
        let v = vp();
        let p = cam.focus;
        let before = cam.project(&v, p).unwrap();
        cam.pan_pixels(&v, Vec2::new(40.0, -25.0));
        let after = cam.project(&v, p).unwrap();
        assert!(((after - before) - Vec2::new(40.0, -25.0)).length() < 0.05, "{:?}", after - before);
    }

    #[test]
    fn pitch_is_clamped_and_frame_contains_box() {
        let mut cam = OrbitCamera::home();
        cam.orbit(0.0, 10.0);
        assert!(cam.pitch < std::f32::consts::FRAC_PI_2);
        let b = Aabb::from_center_size(Vec3::new(10.0, 2.0, -4.0), Vec3::new(4.0, 2.0, 6.0));
        cam.frame(&b);
        let v = vp();
        for c in b.corners() {
            let s = cam.project(&v, c).unwrap();
            assert!(v.contains(s), "corner {c} at {s} is off screen");
        }
    }

    #[test]
    fn front_view_looks_toward_plus_z() {
        let mut cam = OrbitCamera::home();
        cam.set_view(CameraView::Front);
        assert!((cam.forward() - Vec3::Z).length() < 1e-5, "{}", cam.forward());
        cam.set_view(CameraView::Top);
        assert!(cam.forward().y < -0.99);
    }
}
