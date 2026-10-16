//! The application: state plus the per-frame layout.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use ss_doc::{
    Document, Evaluator, History, HistoryEntry, Object, ObjectId, ObjectKind, ProjectTarget, Selection, Shape,
    ShapeSource, ViewStyle,
};
use ss_math::OrbitCamera;
use ss_mesh::{FaceId, PrimitiveKind};
use ss_render::GlRenderer;

use crate::cli::Args;
use crate::screenshot::ScreenshotQueue;
use crate::theme::{self, ThemeChoice};
use crate::viewport::{self, ControlsPreset};

pub const APP_NAME: &str = "Shapeshop";

/// Remembered between runs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub controls: ControlsPreset,
    pub theme: ThemeChoice,
    /// UI size in percent (80..=200).
    pub ui_scale: u32,
    pub reduce_motion: bool,
    pub autosave_secs: u64,
    pub recent: Vec<PathBuf>,
    pub tour_done: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            controls: ControlsPreset::RobloxStudio,
            theme: ThemeChoice::Light,
            ui_scale: 100,
            reduce_motion: false,
            autosave_secs: 120,
            recent: Vec::new(),
            tour_done: false,
        }
    }
}

/// Viewport display state (not part of the document).
#[derive(Clone, Debug)]
pub struct ViewState {
    pub style: ViewStyle,
    pub show_grid: bool,
    pub xray: bool,
    pub hover: Option<ObjectId>,
    pub hover_face: Option<(ObjectId, FaceId)>,
    pub focus_only: Option<Vec<ObjectId>>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self { style: ViewStyle::Colors, show_grid: true, xray: false, hover: None, hover_face: None, focus_only: None }
    }
}

pub struct ShapeshopApp {
    pub args: Args,
    pub prefs: Prefs,
    pub doc: Document,
    pub history: History,
    pub sel: Selection,
    pub camera: OrbitCamera,
    pub eval: Evaluator,
    pub view: ViewState,
    pub renderer: Option<GlRenderer>,
    pub viewport_tex: Option<egui::TextureId>,
    pub viewport_rect: Option<egui::Rect>,
    pub gl_error: Option<String>,
    pub file_path: Option<PathBuf>,
    pub frame_no: u64,
    pub screenshots: ScreenshotQueue,
    pub status: String,
}

impl ShapeshopApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let prefs: Prefs = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, prefs.theme);
        if !args.deterministic {
            cc.egui_ctx.set_zoom_factor(prefs.ui_scale as f32 / 100.0);
        }

        let (renderer, gl_error) = match cc.gl.clone() {
            Some(gl) => match GlRenderer::new(gl, args.msaa) {
                Ok(r) => {
                    log::info!("OpenGL: {} / {}", r.info().renderer, r.info().version);
                    (Some(r), None)
                }
                Err(e) => (None, Some(e)),
            },
            None => (None, Some("No OpenGL context was created.".to_owned())),
        };

        let mut app = Self {
            args: args.clone(),
            prefs,
            doc: Document::new(ProjectTarget::Roblox),
            history: History::new(),
            sel: Selection::default(),
            camera: OrbitCamera::home(),
            eval: Evaluator::new(),
            view: ViewState::default(),
            renderer,
            viewport_tex: None,
            viewport_rect: None,
            gl_error,
            file_path: None,
            frame_no: 0,
            screenshots: ScreenshotQueue::default(),
            status: String::new(),
        };
        if args.demo {
            app.load_demo();
        }
        if let Some(path) = &args.screenshot {
            app.screenshots.request(path.clone(), args.after_frames as u64, true);
        }
        app
    }

    /// A few colored shapes for screenshots and demos.
    pub fn load_demo(&mut self) {
        let mats: Vec<_> = self.doc.materials().map(|(id, _)| id).collect();
        let pick = |name: &str, doc: &Document| {
            doc.materials().find(|(_, m)| m.name == name).map(|(id, _)| id).unwrap_or(mats[0])
        };
        let red = pick("Bright red", &self.doc);
        let green = pick("Bright green", &self.doc);
        let blue = pick("Bright blue", &self.doc);
        let yellow = pick("Bright yellow", &self.doc);
        let items = [
            (PrimitiveKind::Block, red, glam::Vec3::new(0.0, 0.5, 0.0)),
            (PrimitiveKind::Ball, green, glam::Vec3::new(-4.0, 1.0, 1.0)),
            (PrimitiveKind::Cylinder, blue, glam::Vec3::new(4.0, 1.0, 1.0)),
            (PrimitiveKind::Wedge, yellow, glam::Vec3::new(0.0, 0.5, 4.0)),
        ];
        let mut tx = self.doc.begin("Add sample shapes");
        let mut last = None;
        for (kind, mat, pos) in items {
            let name = tx.doc().unique_name(kind.label());
            let mut obj =
                Object::new(name, ObjectKind::Shape(Shape::new(ShapeSource::Primitive(kind.default_primitive()), mat)));
            obj.transform.translation = pos;
            last = tx.add_object(obj, None, None).ok();
        }
        if let Some(delta) = tx.commit() {
            let sel_after = last.map(Selection::single).unwrap_or_default();
            self.history.push(HistoryEntry {
                label: "Add sample shapes".into(),
                invocation: None,
                delta,
                sel_before: Selection::default(),
                sel_after: sel_after.clone(),
                coalesce: None,
            });
            self.sel = sel_after;
            self.history.mark_saved();
        }
    }

    /// Add a starter shape in front of the camera (the Shapes panel buttons).
    pub fn add_shape(&mut self, kind: PrimitiveKind) {
        let Some(mat) = self.doc.default_material() else { return };
        let prim = kind.default_primitive();
        let half_height = prim.size().y * 0.5;
        let mut pos = self.camera.focus;
        pos.y = half_height;
        let snap = self.doc.meta().snap.move_step;
        pos.x = ss_math::snap::snap(pos.x, snap);
        pos.z = ss_math::snap::snap(pos.z, snap);
        let mut tx = self.doc.begin(format!("Add {}", kind.label()));
        let name = tx.doc().unique_name(kind.label());
        let mut obj = Object::new(name, ObjectKind::Shape(Shape::new(ShapeSource::Primitive(prim), mat)));
        obj.transform.translation = pos;
        let Ok(id) = tx.add_object(obj, None, None) else { return };
        let label = tx.label().to_owned();
        if let Some(delta) = tx.commit() {
            let sel_before = self.sel.clone();
            self.sel = Selection::single(id);
            self.history.push(HistoryEntry {
                label,
                invocation: None,
                delta,
                sel_before,
                sel_after: self.sel.clone(),
                coalesce: None,
            });
        }
    }

    pub fn undo(&mut self) {
        if let Some(sel) = self.history.undo(&mut self.doc) {
            self.sel = sel;
            self.sel.validate(&self.doc);
        }
    }

    pub fn redo(&mut self) {
        if let Some(sel) = self.history.redo(&mut self.doc) {
            self.sel = sel;
            self.sel.validate(&self.doc);
        }
    }

    /// Fit the selection (or everything) in view.
    pub fn frame_selection(&mut self) {
        let ids: Vec<ObjectId> = if self.sel.objects.is_empty() {
            self.doc.roots().to_vec()
        } else {
            self.sel.objects.iter().copied().collect()
        };
        let mut b = ss_math::Aabb::EMPTY;
        for id in ids {
            b = b.union(&self.doc.world_bounds(id));
        }
        if !b.is_empty() {
            self.camera.frame(&b);
        }
    }

    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (undo, redo, frame) = ctx.input_mut(|i| {
            let undo = i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z));
            let redo = i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y))
                || i.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ));
            let frame = i.key_pressed(egui::Key::F) && i.modifiers.is_none();
            (undo, redo, frame)
        });
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if frame {
            self.frame_selection();
        }
    }

    fn title_text(&self) -> String {
        let name = self
            .file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.doc.meta().title.clone());
        let state = if self.history.is_dirty() { "Not saved yet" } else { "Saved" };
        format!("{name} — {state}")
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.menu_button("File", |ui| {
                ui.label(RichText::new("Saving and opening arrive in the next update.").weak());
            });
            ui.menu_button("Edit", |ui| {
                if ui.add_enabled(self.history.can_undo(), egui::Button::new("Undo")).clicked() {
                    self.undo();
                }
                if ui.add_enabled(self.history.can_redo(), egui::Button::new("Redo")).clicked() {
                    self.redo();
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label("Hover any button to see what it does.");
            });
            ui.separator();
            let undo_tip =
                self.history.undo_label().map(|l| format!("Undo: {l}  (Ctrl+Z)")).unwrap_or("Nothing to undo".into());
            if ui
                .add_enabled(
                    self.history.can_undo(),
                    egui::Button::new(format!("{} Undo", icon::ARROW_COUNTER_CLOCKWISE)),
                )
                .on_hover_text(undo_tip)
                .on_disabled_hover_text("Nothing to undo yet")
                .clicked()
            {
                self.undo();
            }
            let redo_tip =
                self.history.redo_label().map(|l| format!("Redo: {l}  (Ctrl+Y)")).unwrap_or("Nothing to redo".into());
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new(format!("{} Redo", icon::ARROW_CLOCKWISE)))
                .on_hover_text(redo_tip)
                .on_disabled_hover_text("Nothing to redo")
                .clicked()
            {
                self.redo();
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.status)
                    .hint_text(format!("{}  What do you want to do?   Ctrl+K", icon::MAGNIFYING_GLASS))
                    .desired_width(300.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = egui::Button::new(
                    RichText::new(format!("{} EXPORT", icon::EXPORT)).strong().color(egui::Color32::WHITE),
                )
                .fill(theme::ACCENT);
                ui.add(export).on_hover_text("Send your model to Roblox Studio, Unity, Godot or a 3D printer");
                ui.button(format!("{} Take Picture", icon::CAMERA)).on_hover_text("Save a nice picture of your scene");
                ui.separator();
                ui.label(RichText::new(self.title_text()).strong());
            });
        });
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            theme::section(ui, icon::CUBE, "SHAPES");
            egui::Grid::new("shapes").num_columns(2).spacing([6.0, 6.0]).show(ui, |ui| {
                for (i, kind) in PrimitiveKind::ALL.into_iter().enumerate() {
                    let b = egui::Button::new(kind.label()).min_size(egui::vec2(96.0, 30.0));
                    if ui.add(b).on_hover_text(format!("Add a {} to your scene", kind.label())).clicked() {
                        self.add_shape(kind);
                    }
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.separator();
            theme::section(ui, icon::WRENCH, "TOOLS");
            ui.label(RichText::new("Pick something to see the tools that fit.").weak());
            ui.separator();
            theme::section(ui, icon::PALETTE, "PALETTE");
            ui.horizontal_wrapped(|ui| {
                for (_, m) in self.doc.materials() {
                    let [r, g, b] = m.color.map(|c| (c * 255.0).round() as u8);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 5.0, egui::Color32::from_rgb(r, g, b));
                    resp.on_hover_text(&m.name);
                }
            });
        });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        theme::section(ui, icon::TREE_VIEW, "SCENE");
        egui::ScrollArea::vertical().max_height(ui.available_height() * 0.5).show(ui, |ui| {
            if self.doc.object_count() == 0 {
                ui.label(RichText::new("Your scene is empty. Click a shape on the left to add one.").weak());
            }
            for id in self.doc.walk() {
                let Some(obj) = self.doc.object(id) else { continue };
                let depth = self.doc.ancestors(id).len();
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 14.0);
                    let selected = self.sel.contains(id);
                    if ui.selectable_label(selected, format!("{}  {}", icon::CUBE, obj.name)).clicked() {
                        self.sel.set_only(id);
                    }
                });
            }
        });
        ui.separator();
        theme::section(ui, icon::SLIDERS_HORIZONTAL, "PROPERTIES");
        match self.sel.active().and_then(|id| self.doc.object(id)) {
            Some(obj) => {
                ui.label(RichText::new(&obj.name).strong());
                let t = obj.transform.translation;
                let u = self.doc.meta().units;
                ui.label(format!("Position: {}, {}, {}", u.format(t.x), u.format(t.y), u.format(t.z)));
                if let Some(p) = obj.shape().and_then(|s| s.primitive()) {
                    let s = p.size();
                    ui.label(format!("Size: {} × {} × {}", u.format(s.x), u.format(s.y), u.format(s.z)));
                }
            }
            None => {
                ui.label(RichText::new("Pick something to see its settings.").weak());
            }
        }
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new(format!(
                    "{}  Tip: right-drag to look around, middle-drag to slide, scroll to zoom. F frames what you picked.",
                    icon::LIGHTBULB
                ))
                .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{} steps in History", self.history.cursor())).weak());
            });
        });
    }

    fn gl_error_banner(&mut self, ui: &mut egui::Ui) {
        if let Some(err) = &self.gl_error {
            egui::Frame::new().fill(egui::Color32::from_rgb(0xFF, 0xF1, 0xE6)).inner_margin(8.0).show(ui, |ui| {
                ui.label(RichText::new("3D drawing couldn't start").strong());
                ui.label(err);
            });
        }
    }
}

impl eframe::App for ShapeshopApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.frame_no += 1;
        let ctx = ui.ctx().clone();
        self.handle_global_keys(&ctx);

        egui::Panel::top("top_bar").exact_size(44.0).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom_bar").exact_size(30.0).show(ui, |ui| self.bottom_bar(ui));
        egui::Panel::left("left_panel").resizable(false).exact_size(232.0).show(ui, |ui| self.left_panel(ui));
        egui::Panel::right("right_panel").resizable(false).exact_size(260.0).show(ui, |ui| self.right_panel(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.gl_error_banner(ui);
            viewport::show(self, ui, frame);
        });

        if self.screenshots.update(&ctx, self.frame_no) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.args.deterministic {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.prefs);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(r) = self.renderer.as_mut() {
            r.destroy();
        }
    }
}

/// Shared handle used by tests to build an app without a window.
pub type SharedDoc = Arc<Document>;
