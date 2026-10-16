//! The 3D view: draws the renderer's texture, handles camera gestures and clicks.

use eframe::egui::{self, Pos2, Rect, Sense, Vec2};
use glam::Vec2 as GVec2;
use ss_doc::{RenderScene, SceneOptions};
use ss_math::{CameraView, Viewport};

use crate::app::ShapeshopApp;

/// Camera control presets (what each mouse button does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ControlsPreset {
    /// Right-drag looks around, middle-drag pans, wheel zooms toward the cursor.
    #[default]
    RobloxStudio,
    /// Two-finger scroll orbits, Shift+scroll pans, pinch / Ctrl+scroll zooms.
    Trackpad,
    /// Middle-drag orbits, Shift+middle pans, wheel zooms.
    Blender,
}

impl ControlsPreset {
    pub fn key(self) -> &'static str {
        match self {
            ControlsPreset::RobloxStudio => "roblox",
            ControlsPreset::Trackpad => "trackpad",
            ControlsPreset::Blender => "blender",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [ControlsPreset::RobloxStudio, ControlsPreset::Trackpad, ControlsPreset::Blender]
            .into_iter()
            .find(|c| c.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            ControlsPreset::RobloxStudio => "Roblox Studio",
            ControlsPreset::Trackpad => "Trackpad",
            ControlsPreset::Blender => "Blender",
        }
    }
}

pub fn egui_to_viewport(rect: Rect) -> Viewport {
    Viewport::new(GVec2::new(rect.min.x, rect.min.y), GVec2::new(rect.width(), rect.height()))
}

pub fn to_gvec(p: Pos2) -> GVec2 {
    GVec2::new(p.x, p.y)
}

/// Draw the viewport and handle camera input. Returns the response for further handling.
pub fn show(app: &mut ShapeshopApp, ui: &mut egui::Ui, frame: &mut eframe::Frame) -> egui::Response {
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let vp = egui_to_viewport(rect);
    app.viewport_rect = Some(rect);

    camera_input(app, ui, &response, &vp);

    // Render the scene into our texture and show it.
    let ppp = ui.ctx().pixels_per_point();
    let px = [(rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32];
    let opts = SceneOptions {
        style: app.view.style,
        show_grid: app.view.show_grid,
        xray: app.view.xray,
        hover: app.view.hover,
        hover_face: app.view.hover_face,
        focus_only: app.view.focus_only.clone(),
    };
    let shown = app.preview.as_ref().unwrap_or(&app.doc);
    let scene = RenderScene::build(shown, &mut app.eval, &app.camera, &app.sel, &opts);
    if let Some(renderer) = app.renderer.as_mut() {
        match renderer.render_viewport(&scene, px[0], px[1]) {
            Ok(tex) => {
                let id = *app.viewport_tex.get_or_insert_with(|| frame.register_native_glow_texture(tex));
                // GL textures are bottom-up.
                let uv = Rect::from_min_max(Pos2::new(0.0, 1.0), Pos2::new(1.0, 0.0));
                ui.painter().image(id, rect, uv, egui::Color32::WHITE);
            }
            Err(e) => {
                app.gl_error.get_or_insert(e);
            }
        }
    } else {
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(200));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "3D drawing isn't available on this computer.",
            egui::FontId::proportional(16.0),
            egui::Color32::BLACK,
        );
    }
    crate::gestures::handle(app, ui, &response, rect, &vp);
    view_buttons(app, ui, rect);
    response
}

fn camera_input(app: &mut ShapeshopApp, ui: &egui::Ui, response: &egui::Response, vp: &Viewport) {
    let ctx = ui.ctx();
    let hovered = response.hovered() || response.dragged();
    let (scroll, zoom_delta, modifiers, pointer, keys) = ctx.input(|i| {
        (
            i.smooth_scroll_delta,
            i.zoom_delta(),
            i.modifiers,
            i.pointer.clone(),
            (
                i.key_down(egui::Key::W),
                i.key_down(egui::Key::A),
                i.key_down(egui::Key::S),
                i.key_down(egui::Key::D),
                i.key_down(egui::Key::Q),
                i.key_down(egui::Key::E),
            ),
        )
    });
    let delta = pointer.delta();
    let orbit_speed = 0.008;
    let cursor = pointer.hover_pos();

    match app.prefs.controls {
        ControlsPreset::RobloxStudio => {
            if response.dragged_by(egui::PointerButton::Secondary) {
                app.camera.orbit(delta.x * orbit_speed, delta.y * orbit_speed);
                // Fly with WASD/QE while right-dragging, like Roblox Studio.
                let (w, a, s, d, q, e) = keys;
                let speed = app.camera.distance.max(4.0) * 0.02 * if modifiers.shift { 0.25 } else { 1.0 };
                let mv = glam::Vec3::new(
                    (d as i32 - a as i32) as f32,
                    (e as i32 - q as i32) as f32,
                    (w as i32 - s as i32) as f32,
                ) * speed;
                if mv != glam::Vec3::ZERO {
                    app.camera.fly(mv);
                    ctx.request_repaint();
                }
            }
            if response.dragged_by(egui::PointerButton::Middle) {
                app.camera.pan_pixels(vp, GVec2::new(delta.x, delta.y));
            }
            if hovered && scroll.y != 0.0 {
                zoom_at_cursor(app, vp, cursor, (scroll.y * 0.0025).exp());
            }
        }
        ControlsPreset::Trackpad => {
            if hovered && (scroll.x != 0.0 || scroll.y != 0.0) && zoom_delta == 1.0 {
                if modifiers.shift {
                    app.camera.pan_pixels(vp, GVec2::new(scroll.x, scroll.y));
                } else if modifiers.command || modifiers.ctrl {
                    zoom_at_cursor(app, vp, cursor, (scroll.y * 0.004).exp());
                } else {
                    app.camera.orbit(-scroll.x * orbit_speed, -scroll.y * orbit_speed);
                }
            }
            if response.dragged_by(egui::PointerButton::Secondary) {
                app.camera.orbit(delta.x * orbit_speed, delta.y * orbit_speed);
            }
            if response.dragged_by(egui::PointerButton::Middle) {
                app.camera.pan_pixels(vp, GVec2::new(delta.x, delta.y));
            }
        }
        ControlsPreset::Blender => {
            if response.dragged_by(egui::PointerButton::Middle) {
                if modifiers.shift {
                    app.camera.pan_pixels(vp, GVec2::new(delta.x, delta.y));
                } else {
                    app.camera.orbit(delta.x * orbit_speed, delta.y * orbit_speed);
                }
            }
            if hovered && scroll.y != 0.0 {
                zoom_at_cursor(app, vp, cursor, (scroll.y * 0.0025).exp());
            }
        }
    }
    if hovered && zoom_delta != 1.0 {
        zoom_at_cursor(app, vp, cursor, zoom_delta);
    }
}

/// Zoom so the point under the cursor stays under the cursor.
fn zoom_at_cursor(app: &mut ShapeshopApp, vp: &Viewport, cursor: Option<Pos2>, factor: f32) {
    let target = cursor.filter(|c| vp.contains(to_gvec(*c))).and_then(|c| {
        let ray = app.camera.ray_through(vp, to_gvec(c));
        ss_doc::pick_object(&app.doc, &mut app.eval, &ray, |_| true).map(|h| h.point).or_else(|| {
            ray.intersect_plane(&ss_math::Plane::from_point_normal(glam::Vec3::ZERO, glam::Vec3::Y)).map(|t| ray.at(t))
        })
    });
    app.camera.zoom(factor, target);
}

/// The view area: named views, Home and Frame, view toggles and on-screen
/// look-around/zoom buttons (for trackpads and anyone without a middle button).
fn view_buttons(app: &mut ShapeshopApp, ui: &mut egui::Ui, rect: Rect) {
    use egui_phosphor::regular as icon;
    let size = Vec2::new(204.0, 190.0);
    // On narrow windows the Tool strip needs the whole top row, so drop below it.
    let top = if rect.width() < 1060.0 { rect.min.y + 60.0 } else { rect.min.y + 10.0 };
    let area = Rect::from_min_size(Pos2::new(rect.max.x - size.x - 10.0, top), size);
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::Frame::popup(child.style()).inner_margin(6.0).show(&mut child, |ui| {
        ui.spacing_mut().button_padding = Vec2::new(6.0, 3.0);
        ui.horizontal(|ui| {
            for (label, tip, view, anchor) in [
                ("Top", "Look straight down", CameraView::Top, "view.top"),
                ("Front", "Look at the front of your model", CameraView::Front, "view.front"),
                ("Side", "Look from the right side", CameraView::Right, "view.side"),
            ] {
                let r = ui.small_button(label).on_hover_text(tip);
                app.anchors.register(anchor, r.rect);
                if r.clicked() {
                    app.camera.set_view(view);
                }
            }
        });
        ui.horizontal(|ui| {
            let r = ui.small_button(format!("{} Home", icon::HOUSE)).on_hover_text("Go back to the starting view");
            app.anchors.register("view.home", r.rect);
            if r.clicked() {
                app.camera.set_view(CameraView::Home);
            }
            let r = ui
                .small_button(format!("{} Frame", icon::FRAME_CORNERS))
                .on_hover_text("Fit what you picked (or everything) in the view  (F)");
            app.anchors.register("view.frame", r.rect);
            if r.clicked() {
                app.frame_selection();
            }
        });
        ui.horizontal(|ui| {
            let r = ui
                .selectable_label(app.camera.ortho, "Flat")
                .on_hover_text("Flat view: no perspective, so sizes line up (Orthographic)");
            app.anchors.register("view.flat", r.rect);
            if r.clicked() {
                app.camera.ortho = !app.camera.ortho;
            }
            let r =
                ui.selectable_label(app.view.xray, "X-ray").on_hover_text("See through shapes to what's behind them");
            app.anchors.register("view.xray", r.rect);
            if r.clicked() {
                app.view.xray = !app.view.xray;
            }
            let r = ui.selectable_label(app.view.show_grid, "Grid").on_hover_text("Show or hide the floor grid");
            if r.clicked() {
                app.view.show_grid = !app.view.show_grid;
            }
        });
        ui.horizontal(|ui| {
            let step = 15f32.to_radians();
            for (ic, tip, yaw, pitch) in [
                (icon::ARROW_LEFT, "Look around to the left", -step, 0.0),
                (icon::ARROW_RIGHT, "Look around to the right", step, 0.0),
                (icon::ARROW_UP, "Look from higher up", 0.0, step),
                (icon::ARROW_DOWN, "Look from lower down", 0.0, -step),
            ] {
                if ui.small_button(ic).on_hover_text(tip).clicked() {
                    app.camera.orbit(yaw, pitch);
                }
            }
            if ui.small_button(icon::MAGNIFYING_GLASS_PLUS).on_hover_text("Zoom in").clicked() {
                app.camera.zoom(1.25, None);
            }
            if ui.small_button(icon::MAGNIFYING_GLASS_MINUS).on_hover_text("Zoom out").clicked() {
                app.camera.zoom(0.8, None);
            }
        });
    });
}
