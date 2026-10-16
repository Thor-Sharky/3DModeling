//! The application: state plus the per-frame layout.

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use ss_doc::{
    DocError, Document, Evaluator, History, HistoryEntry, Object, ObjectId, ObjectKind, PickFilter, ProjectTarget,
    Selection, Shape, ShapeSource, Tx, ViewStyle,
};
use ss_math::{CameraView, OrbitCamera};
use ss_mesh::{FaceId, PrimitiveKind};
use ss_ops::{ActiveTool, AppAction, CameraOp, SelectOp};
use ss_render::GlRenderer;
use transform_gizmo_egui::Gizmo;

use crate::anchors::Anchors;
use crate::cli::Args;
use crate::gestures::DragSession;
use crate::screenshot::ScreenshotQueue;
use crate::script::ScriptRunner;
use crate::theme::{self, ThemeChoice};
use crate::viewport::{self, ControlsPreset};

pub const APP_NAME: &str = "Shapeshop";

/// Round shapes start smooth (flat caps stay crisp); boxy ones start faceted.
pub fn default_look(kind: PrimitiveKind) -> ss_mesh::Shading {
    match kind {
        PrimitiveKind::Ball
        | PrimitiveKind::Cylinder
        | PrimitiveKind::Cone
        | PrimitiveKind::Ring
        | PrimitiveKind::Tube => ss_mesh::Shading::SmoothSharp { angle_deg: 40.0 },
        _ => ss_mesh::Shading::Faceted,
    }
}

/// Exit code set by scripts (0 = passed, 3 = failed).
pub static EXIT_CODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

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

/// A short message in the corner ("Locked in so you can edit faces...").
#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub until: f64,
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
    pub tool: ActiveTool,
    /// Move/Turn along the object's own axes instead of the world's.
    pub local_axes: bool,
    pub gizmo: Gizmo,
    /// A drag in progress (gizmo, size handle, box select).
    pub drag: Option<DragSession>,
    /// What to draw instead of `doc` while a drag is previewed.
    pub preview: Option<Document>,
    pub anchors: Anchors,
    pub renderer: Option<GlRenderer>,
    pub viewport_tex: Option<egui::TextureId>,
    pub viewport_rect: Option<egui::Rect>,
    pub pixels_per_point: f32,
    pub gl_error: Option<String>,
    pub file_path: Option<PathBuf>,
    pub frame_no: u64,
    pub time: f64,
    pub screenshots: ScreenshotQueue,
    pub script: Option<ScriptRunner>,
    pub search_text: String,
    pub toasts: Vec<Toast>,
    /// Set when something this frame already used the Esc key.
    pub esc_consumed: bool,
    /// Changes while one widget is being dragged share this number (so they merge).
    pub edit_session: u64,
    pub welcome: crate::welcome::WelcomeState,
    pub outliner: crate::outliner::OutlinerState,
    pub settings: crate::welcome::SettingsState,
    /// Set by the UI so `dispatch` can reach the egui context for settings.
    pub ctx: Option<egui::Context>,
}

impl ShapeshopApp {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        let prefs: Prefs = if args.deterministic {
            Prefs::default()
        } else {
            cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default()
        };
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

        let script = args.script.as_ref().map(|path| {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "script".into());
            let out = PathBuf::from("target/shots").join(stem);
            ScriptRunner::load(path, out).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2);
            })
        });

        let mut app = Self {
            args: args.clone(),
            prefs,
            doc: Document::new(ProjectTarget::Roblox),
            history: History::new(),
            sel: Selection::default(),
            camera: OrbitCamera::home(),
            eval: Evaluator::new(),
            view: ViewState::default(),
            tool: ActiveTool::Select,
            local_axes: false,
            gizmo: Gizmo::default(),
            drag: None,
            preview: None,
            anchors: Anchors::default(),
            renderer,
            viewport_tex: None,
            viewport_rect: None,
            pixels_per_point: 1.0,
            gl_error,
            file_path: None,
            frame_no: 0,
            time: 0.0,
            screenshots: ScreenshotQueue::default(),
            script,
            search_text: String::new(),
            toasts: Vec::new(),
            esc_consumed: false,
            edit_session: 0,
            welcome: Default::default(),
            outliner: Default::default(),
            settings: Default::default(),
            ctx: Some(cc.egui_ctx.clone()),
        };
        // First run: ask what they're making (scripts and screenshots skip it).
        app.welcome.open = !args.deterministic && !app.prefs.tour_done && args.open.is_none();
        if args.demo {
            app.load_demo();
        }
        if let Some(path) = &args.screenshot {
            app.screenshots.request(path.clone(), args.after_frames as u64, true);
        }
        app
    }

    /// The document as shown (the preview while dragging).
    pub fn shown_doc(&self) -> &Document {
        self.preview.as_ref().unwrap_or(&self.doc)
    }

    /// A few colored shapes for screenshots and demos.
    pub fn load_demo(&mut self) {
        let pick = |name: &str, doc: &Document| {
            doc.materials().find(|(_, m)| m.name == name).map(|(id, _)| id).or(doc.default_material())
        };
        let items = [
            (PrimitiveKind::Block, "Bright red", glam::Vec3::new(0.0, 0.5, 0.0)),
            (PrimitiveKind::Ball, "Bright green", glam::Vec3::new(-4.0, 1.0, 1.0)),
            (PrimitiveKind::Cylinder, "Bright blue", glam::Vec3::new(4.0, 1.0, 1.0)),
            (PrimitiveKind::Wedge, "Bright yellow", glam::Vec3::new(0.0, 0.5, 4.0)),
        ];
        let mut last = None;
        self.commit_with("Add sample shapes", None, |tx| {
            for (kind, color, pos) in items {
                let mat = pick(color, tx.doc()).ok_or(DocError::Invalid("no colors".into()))?;
                let name = tx.doc().unique_name(kind.label());
                let mut obj = Object::new(
                    name,
                    ObjectKind::Shape(Shape::new(ShapeSource::Primitive(kind.default_primitive()), mat)),
                );
                obj.transform.translation = pos;
                last = Some(tx.add_object(obj, None, None)?);
            }
            Ok(())
        });
        if let Some(id) = last {
            self.sel = Selection::single(id);
        }
        self.history.mark_saved();
    }

    /// Add a starter shape near the center of the view, resting on the ground,
    /// in the nearest free spot so new shapes don't land inside each other.
    pub fn add_shape(&mut self, kind: PrimitiveKind) {
        let Some(mat) = self.doc.default_material() else { return };
        let prim = kind.default_primitive();
        let pos = self.free_spot(prim.size());
        let mut new_id = None;
        self.commit_with(&format!("Add {}", kind.label()), None, |tx| {
            let name = tx.doc().unique_name(kind.label());
            let mut shape = Shape::new(ShapeSource::Primitive(prim), mat);
            shape.look = default_look(kind);
            let mut obj = Object::new(name, ObjectKind::Shape(shape));
            obj.transform.translation = pos;
            new_id = Some(tx.add_object(obj, None, None)?);
            Ok(())
        });
        if let Some(id) = new_id {
            self.set_selection(Selection::single(id));
        }
    }

    /// Where a new shape of `size` goes: on the ground under the camera focus, or the
    /// nearest grid spot (spiralling outward) where it doesn't overlap anything.
    pub fn free_spot(&self, size: glam::Vec3) -> glam::Vec3 {
        let step = self.doc.meta().snap.move_step.max(0.25);
        let snap = |v: f32| ss_math::snap::snap(v, step);
        let center = glam::Vec3::new(snap(self.camera.focus.x), size.y * 0.5, snap(self.camera.focus.z));
        let others: Vec<ss_math::Aabb> =
            self.doc.roots().iter().map(|id| self.doc.world_bounds(*id)).filter(|b| !b.is_empty()).collect();
        let gap = 0.5;
        let fits = |p: glam::Vec3| {
            let b = ss_math::Aabb::from_center_size(p, size + glam::Vec3::splat(gap));
            others.iter().all(|o| {
                !(b.min.x < o.max.x
                    && b.max.x > o.min.x
                    && b.min.y < o.max.y
                    && b.max.y > o.min.y
                    && b.min.z < o.max.z
                    && b.max.z > o.min.z)
            })
        };
        let spacing = (size.x.max(size.z) + gap).max(step);
        for ring in 0..12i32 {
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs() != ring && dz.abs() != ring {
                        continue;
                    }
                    let p = center + glam::Vec3::new(snap(dx as f32 * spacing), 0.0, snap(dz as f32 * spacing));
                    if fits(p) {
                        return p;
                    }
                }
            }
        }
        center
    }

    /// Start a fresh project (the Welcome card's "Start building").
    pub fn new_project(&mut self, meta: ss_doc::DocMeta) {
        let mut doc = Document::new(meta.target);
        {
            let mut tx = doc.begin("Setup");
            *tx.meta_mut() = meta;
            tx.commit();
        }
        self.doc = doc;
        self.history.clear();
        self.sel = Selection::default();
        self.eval.clear();
        self.preview = None;
        self.drag = None;
        self.file_path = None;
        self.camera = OrbitCamera::home();
    }

    /// Delete the picked objects.
    pub fn delete_selected(&mut self) {
        let ids = crate::gestures::manipulable(&self.doc, &self.sel);
        if ids.is_empty() {
            return;
        }
        let label = if ids.len() == 1 { "Delete" } else { "Delete objects" };
        self.commit_with(label, Some(Selection::default()), |tx| {
            for id in &ids {
                if tx.doc().contains(*id) {
                    tx.remove_object(*id)?;
                }
            }
            Ok(())
        });
    }

    /// Run a change as one undoable step. `sel_after` replaces the selection if given.
    /// Returns true if something changed.
    pub fn commit_with(
        &mut self,
        label: &str,
        sel_after: Option<Selection>,
        f: impl FnOnce(&mut Tx<'_>) -> Result<(), DocError>,
    ) -> bool {
        self.commit_inner(label, sel_after, None, f)
    }

    /// Like [`Self::commit_with`], but consecutive changes with the same key merge into one
    /// history step (dragging a number or a slider).
    pub fn commit_coalesced(
        &mut self,
        label: &str,
        key: u64,
        f: impl FnOnce(&mut Tx<'_>) -> Result<(), DocError>,
    ) -> bool {
        let key = key ^ self.edit_session.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.commit_inner(label, None, Some(key), f)
    }

    fn commit_inner(
        &mut self,
        label: &str,
        sel_after: Option<Selection>,
        coalesce: Option<u64>,
        f: impl FnOnce(&mut Tx<'_>) -> Result<(), DocError>,
    ) -> bool {
        let sel_before = self.sel.clone();
        let mut tx = self.doc.begin(label);
        match f(&mut tx) {
            Ok(()) => {
                let Some(delta) = tx.commit() else { return false };
                let mut after = sel_after.unwrap_or_else(|| sel_before.clone());
                after.validate(&self.doc);
                self.sel = after.clone();
                self.history.push(HistoryEntry {
                    label: label.to_owned(),
                    invocation: None,
                    delta,
                    sel_before,
                    sel_after: after,
                    coalesce,
                });
                true
            }
            Err(e) => {
                tx.abort();
                log::warn!("{label} failed: {e}");
                self.toast(format!("{label} didn't work: {e}"));
                false
            }
        }
    }

    pub fn set_selection(&mut self, mut sel: Selection) {
        sel.filter = self.sel.filter;
        sel.validate(&self.doc);
        self.sel = sel;
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), until: self.time + 4.0 });
    }

    pub fn status_text(&self) -> String {
        self.toasts.last().map(|t| t.text.clone()).unwrap_or_default()
    }

    pub fn undo(&mut self) {
        self.cancel_drag();
        if let Some(sel) = self.history.undo(&mut self.doc) {
            self.sel = sel;
            self.sel.validate(&self.doc);
        }
    }

    pub fn redo(&mut self) {
        self.cancel_drag();
        if let Some(sel) = self.history.redo(&mut self.doc) {
            self.sel = sel;
            self.sel.validate(&self.doc);
        }
    }

    pub fn cancel_drag(&mut self) {
        if let Some(d) = self.drag.as_mut() {
            d.cancelled = true;
        }
        self.preview = None;
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

    /// Perform an action from a button, shortcut, script, quest or search result.
    pub fn dispatch(&mut self, action: AppAction) {
        match action {
            AppAction::SetTool(t) => self.tool = t,
            AppAction::SetFilter(f) => {
                self.sel.filter = f;
                self.sel.elements = Default::default();
            }
            AppAction::Select(op) => {
                let sel = match op {
                    SelectOp::None => Selection::default(),
                    SelectOp::All => {
                        let mut s = Selection::default();
                        s.objects.extend(self.doc.roots().iter().copied());
                        s
                    }
                    SelectOp::Only(id) => Selection::single(id),
                    SelectOp::Toggle(id) => {
                        let mut s = self.sel.clone();
                        s.toggle(id);
                        s
                    }
                    SelectOp::ByName(name) => self
                        .doc
                        .objects()
                        .find(|(_, o)| o.name == name)
                        .map(|(id, _)| Selection::single(id))
                        .unwrap_or_default(),
                };
                self.set_selection(sel);
            }
            AppAction::Undo => self.undo(),
            AppAction::Redo => self.redo(),
            AppAction::JumpHistory(n) => {
                self.cancel_drag();
                if let Some(sel) = self.history.jump_to(n, &mut self.doc) {
                    self.sel = sel;
                    self.sel.validate(&self.doc);
                }
            }
            AppAction::Camera(op) => match op {
                CameraOp::Home => self.camera.set_view(CameraView::Home),
                CameraOp::Frame => self.frame_selection(),
                CameraOp::View(v) => self.camera.set_view(v),
                CameraOp::Orbit { yaw, pitch } => self.camera.orbit(yaw, pitch),
                CameraOp::Zoom(f) => self.camera.zoom(f, None),
                CameraOp::ToggleFlat => self.camera.ortho = !self.camera.ortho,
                CameraOp::ToggleXRay => self.view.xray = !self.view.xray,
            },
            AppAction::RunTool { id, params } => {
                if let Some(kind) = id.strip_prefix("shape.add.").and_then(PrimitiveKind::from_key) {
                    self.add_shape(kind);
                } else if id == "object.delete" {
                    self.delete_selected();
                } else if let Some(field) = id.strip_prefix("object.set_").filter(|f| *f == "visible" || *f == "locked")
                {
                    let value = params.get("value").and_then(|v| v.as_bool()).unwrap_or(true);
                    let ids: Vec<ObjectId> = self.sel.objects.iter().copied().collect();
                    let label = match (field, value) {
                        ("visible", true) => "Show",
                        ("visible", false) => "Hide",
                        (_, true) => "Lock",
                        (_, false) => "Unlock",
                    };
                    self.commit_with(label, None, |tx| {
                        for id in &ids {
                            let o = tx.object_mut(*id)?;
                            if field == "visible" { o.visible = value } else { o.locked = value }
                        }
                        Ok(())
                    });
                } else if id == "object.rename" {
                    if let (Some(active), Some(name)) = (self.sel.active(), params.get("name").and_then(|v| v.as_str()))
                    {
                        let name = name.to_owned();
                        self.commit_with("Rename", None, |tx| {
                            tx.object_mut(active)?.name = name;
                            Ok(())
                        });
                    }
                } else {
                    self.toast(format!("{id} isn't wired up yet"));
                }
            }
            AppAction::SetSetting { key, value } => {
                if let Some(ctx) = self.ctx.clone()
                    && let Err(e) = crate::welcome::apply_setting(self, &ctx, &key, &value)
                {
                    log::warn!("{e}");
                }
            }
            AppAction::OpenSettings => self.settings.open = true,
            AppAction::File(ss_ops::FileOp::New) => {
                self.welcome = crate::welcome::WelcomeState { open: true, ..Default::default() };
            }
            other => self.toast(format!("{other:?} isn't wired up yet")),
        }
    }

    /// Esc backs out one step at a time: cancel a drag, then clear the pick, then go back to Select.
    fn escape_ladder(&mut self) {
        if self.esc_consumed {
            return;
        }
        if self.drag.is_some() {
            self.cancel_drag();
        } else if !self.sel.is_empty() {
            self.set_selection(Selection::default());
        } else {
            self.tool = ActiveTool::Select;
            self.sel.filter = PickFilter::Objects;
        }
    }

    fn handle_global_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::{Key, KeyboardShortcut as Ks, Modifiers as M};
        let mut actions = Vec::new();
        ctx.input_mut(|i| {
            let mut sc = |m: M, k: Key, a: AppAction| {
                if i.consume_shortcut(&Ks::new(m, k)) {
                    actions.push(a);
                }
            };
            sc(M::COMMAND | M::SHIFT, Key::Z, AppAction::Redo);
            sc(M::COMMAND, Key::Z, AppAction::Undo);
            sc(M::COMMAND, Key::Y, AppAction::Redo);
            sc(M::COMMAND, Key::Num1, AppAction::SetTool(ActiveTool::Select));
            sc(M::COMMAND, Key::Num2, AppAction::SetTool(ActiveTool::Move));
            sc(M::COMMAND, Key::Num3, AppAction::SetTool(ActiveTool::Size));
            sc(M::COMMAND, Key::Num4, AppAction::SetTool(ActiveTool::Turn));
            sc(M::NONE, Key::Num1, AppAction::SetFilter(PickFilter::Objects));
            sc(M::NONE, Key::Num2, AppAction::SetFilter(PickFilter::Faces));
            sc(M::NONE, Key::F, AppAction::Camera(CameraOp::Frame));
            sc(M::NONE, Key::Delete, AppAction::run("object.delete"));
            sc(M::NONE, Key::Backspace, AppAction::run("object.delete"));
            if i.consume_shortcut(&Ks::new(M::COMMAND, Key::L)) {
                actions.push(AppAction::SetSetting { key: "local_axes".into(), value: "toggle".into() });
            }
        });
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        for a in actions {
            self.dispatch(a);
        }
        if esc {
            self.escape_ladder();
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
                if ui.button(format!("{} Settings", icon::GEAR)).clicked() {
                    self.dispatch(AppAction::OpenSettings);
                }
                if ui.button(format!("{} Welcome card", icon::HAND_WAVING)).clicked() {
                    self.welcome.open = true;
                }
            });
            ui.separator();
            let undo_tip =
                self.history.undo_label().map(|l| format!("Undo: {l}  (Ctrl+Z)")).unwrap_or("Nothing to undo".into());
            let r = ui
                .add_enabled(
                    self.history.can_undo(),
                    egui::Button::new(format!("{} Undo", icon::ARROW_COUNTER_CLOCKWISE)),
                )
                .on_hover_text(undo_tip)
                .on_disabled_hover_text("Nothing to undo yet");
            self.anchors.register("top.undo", r.rect);
            if r.clicked() {
                self.undo();
            }
            let redo_tip =
                self.history.redo_label().map(|l| format!("Redo: {l}  (Ctrl+Y)")).unwrap_or("Nothing to redo".into());
            let r = ui
                .add_enabled(self.history.can_redo(), egui::Button::new(format!("{} Redo", icon::ARROW_CLOCKWISE)))
                .on_hover_text(redo_tip)
                .on_disabled_hover_text("Nothing to redo");
            self.anchors.register("top.redo", r.rect);
            if r.clicked() {
                self.redo();
            }
            ui.separator();
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.search_text)
                    .hint_text(format!("{}  What do you want to do?   Ctrl+K", icon::MAGNIFYING_GLASS))
                    .desired_width(300.0),
            );
            self.anchors.register("top.search", r.rect);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = egui::Button::new(
                    RichText::new(format!("{} EXPORT", icon::EXPORT)).strong().color(egui::Color32::WHITE),
                )
                .fill(theme::ACCENT);
                let r = ui.add(export).on_hover_text("Send your model to Roblox Studio, Unity, Godot or a 3D printer");
                self.anchors.register("top.export", r.rect);
                let r = ui
                    .button(format!("{} Take Picture", icon::CAMERA))
                    .on_hover_text("Save a nice picture of your scene");
                self.anchors.register("top.picture", r.rect);
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
                    let r = ui.add(b).on_hover_text(format!("Add a {} to your scene", kind.label()));
                    self.anchors.register(format!("shapes.{}", kind.key()), r.rect);
                    if r.clicked() {
                        self.dispatch(AppAction::run(format!("shape.add.{}", kind.key())));
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
                for (id, m) in self.doc.materials() {
                    let [r, g, b] = m.color.map(|c| (c * 255.0).round() as u8);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 5.0, egui::Color32::from_rgb(r, g, b));
                    self.anchors.register(format!("palette.{}", id.0), rect);
                    resp.on_hover_text(&m.name);
                }
            });
        });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        crate::outliner::show(self, ui);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("props").max_height(ui.available_height() - 200.0).show(ui, |ui| {
            crate::panels::properties(self, ui);
        });
        ui.separator();
        crate::panels::history(self, ui);
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let hint = match self.tool {
                ActiveTool::Select => "Click a shape to pick it. Drag on empty space to pick several. Esc clears.",
                ActiveTool::Move => {
                    "Drag an arrow to move along it, or a square to slide flat. Shift = no snapping. Esc cancels."
                }
                ActiveTool::Size => "Drag a dot to grow or shrink that side. Shift = no snapping. Esc cancels.",
                ActiveTool::Turn => "Drag a ring to turn around it. Shift = no snapping. Esc cancels.",
            };
            ui.label(RichText::new(format!("{}  {hint}", icon::LIGHTBULB)).weak());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("{} steps in History", self.history.cursor())).weak());
            });
        });
    }

    /// Tool strip and Pick filter, floating at the top of the viewport.
    fn tool_strip(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let area = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(rect.width() - 200.0, 40.0));
        let mut child = ui
            .new_child(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::left_to_right(egui::Align::Center)));
        egui::Frame::popup(child.style()).inner_margin(egui::Margin::symmetric(8, 4)).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Tool").weak());
                let tools = [
                    (ActiveTool::Select, icon::CURSOR, "Ctrl+1", "Click to pick things."),
                    (ActiveTool::Move, icon::ARROWS_OUT_CARDINAL, "Ctrl+2", "Drag the arrows to move what you picked."),
                    (ActiveTool::Size, icon::ARROWS_OUT, "Ctrl+3", "Drag the dots to make one side bigger or smaller."),
                    (ActiveTool::Turn, icon::ARROWS_CLOCKWISE, "Ctrl+4", "Drag the rings to turn what you picked."),
                ];
                for (t, ic, key, tip) in tools {
                    let r = ui.selectable_label(self.tool == t, format!("{ic} {}", t.label())).on_hover_text(format!(
                        "{} ({})  {key}\n{tip}",
                        t.label(),
                        t.term()
                    ));
                    self.anchors.register(format!("tool.{}", t.label().to_lowercase()), r.rect);
                    if r.clicked() {
                        self.dispatch(AppAction::SetTool(t));
                    }
                }
                ui.separator();
                ui.label(RichText::new("Pick").weak());
                for f in PickFilter::ALL {
                    let available = matches!(f, PickFilter::Objects);
                    let r = ui
                        .add_enabled(available, egui::Button::selectable(self.sel.filter == f, f.label()))
                        .on_hover_text(format!("Clicks pick {}", f.label().to_lowercase()))
                        .on_disabled_hover_text("Picking faces, edges and points arrives in the next update.");
                    self.anchors.register(format!("filter.{}", f.label().to_lowercase()), r.rect);
                    if r.clicked() {
                        self.dispatch(AppAction::SetFilter(f));
                    }
                }
                ui.separator();
                let r = ui
                    .selectable_label(self.local_axes, if self.local_axes { "Object axes" } else { "World axes" })
                    .on_hover_text("Move and turn along the world's directions, or the object's own.  Ctrl+L");
                if r.clicked() {
                    self.local_axes = !self.local_axes;
                }
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

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.until > self.time);
        if let (Some(t), Some(rect)) = (self.toasts.last(), self.viewport_rect) {
            let pos = egui::pos2(rect.center().x, rect.max.y - 60.0);
            egui::Area::new(egui::Id::new("toast")).fixed_pos(pos).pivot(egui::Align2::CENTER_BOTTOM).show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(&t.text);
                });
            });
        }
    }
}

impl eframe::App for ShapeshopApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(script) = self.script.as_mut() {
            script.inject(raw_input);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.frame_no += 1;
        self.esc_consumed = false;
        self.anchors.begin_frame();
        let ctx = ui.ctx().clone();
        if ctx.dragged_id().is_none() && !ctx.egui_wants_keyboard_input() {
            self.edit_session += 1;
        }
        self.time = ctx.input(|i| i.time);
        self.pixels_per_point = ctx.pixels_per_point();

        egui::Panel::top("top_bar").exact_size(44.0).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("bottom_bar").exact_size(30.0).show(ui, |ui| self.bottom_bar(ui));
        egui::Panel::left("left_panel").resizable(false).exact_size(232.0).show(ui, |ui| self.left_panel(ui));
        egui::Panel::right("right_panel").resizable(false).exact_size(260.0).show(ui, |ui| self.right_panel(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| {
            self.gl_error_banner(ui);
            let rect = ui.available_rect_before_wrap();
            viewport::show(self, ui, frame);
            self.tool_strip(ui, rect);
        });
        self.handle_global_keys(&ctx);
        self.draw_toasts(&ctx);
        crate::welcome::show_welcome(self, &ctx);
        crate::welcome::show_settings(self, &ctx);

        if let Some(mut script) = self.script.take() {
            script.tick(self, &ctx);
            if let Some(passed) = script.finished {
                match script.write_report() {
                    Ok(p) => log::info!("script report: {}", p.display()),
                    Err(e) => log::error!("could not write the script report: {e}"),
                }
                EXIT_CODE.store(if passed { 0 } else { 3 }, std::sync::atomic::Ordering::SeqCst);
                if !self.screenshots.is_busy() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            self.script = Some(script);
        }
        if self.screenshots.update(&ctx, self.frame_no) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.args.deterministic {
            ctx.request_repaint();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.args.deterministic {
            eframe::set_value(storage, eframe::APP_KEY, &self.prefs);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(r) = self.renderer.as_mut() {
            r.destroy();
        }
    }
}
