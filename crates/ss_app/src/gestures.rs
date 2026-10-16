//! Direct manipulation in the 3D view: picking, hover, box select, the Move/Turn
//! gizmo and Roblox-style one-sided Size handles.
//!
//! While a drag is in progress the change is shown on a cloned preview document;
//! releasing commits it as one history step, Esc throws it away.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use glam::{DMat4, DQuat, DVec3, Mat3, Vec3};
use ss_doc::{Document, ObjectId, Selection, ShapeSource, Tx};
use ss_math::{Ray, Transform, Viewport};
use ss_ops::ActiveTool;
use transform_gizmo_egui::prelude::*;

use crate::app::ShapeshopApp;
use crate::theme;
use crate::viewport::to_gvec;

/// A drag that is being previewed.
pub struct DragSession {
    pub kind: DragKind,
    /// The document when the drag started; previews are built from it.
    pub start_doc: Document,
    pub cancelled: bool,
}

#[derive(Clone, Debug)]
pub enum DragKind {
    Gizmo { ids: Vec<ObjectId>, turn: bool },
    Size(SizeDrag),
    BoxSelect { start: Pos2, additive: bool },
}

#[derive(Clone, Debug)]
pub struct SizeDrag {
    pub id: ObjectId,
    /// Local axis index (0 = X, 1 = Y, 2 = Z) and which side (+1 / -1).
    pub axis: usize,
    pub side: f32,
    /// World-space line the handle moves along.
    pub line: Ray,
    /// Parameter on `line` where the pointer grabbed the handle.
    pub grab_t: f32,
    pub start_len: f32,
    pub new_len: f32,
}

/// One of the six size handles of the active object.
#[derive(Clone, Copy, Debug)]
pub struct SizeHandle {
    pub axis: usize,
    pub side: f32,
    pub world: Vec3,
    pub screen: Pos2,
    pub dir: Vec3,
}

const HANDLE_RADIUS: f32 = 9.0;

/// Objects the gizmo moves: picked, unlocked, and not inside another picked object.
pub fn manipulable(doc: &Document, sel: &Selection) -> Vec<ObjectId> {
    sel.objects
        .iter()
        .copied()
        .filter(|id| doc.contains(*id) && !doc.is_locked(*id))
        .filter(|id| !doc.ancestors(*id).iter().any(|a| sel.contains(*a)))
        .collect()
}

pub fn handle(app: &mut ShapeshopApp, ui: &mut egui::Ui, response: &egui::Response, rect: Rect, vp: &Viewport) {
    let pointer = ui.input(|i| i.pointer.clone());
    let shift = ui.input(|i| i.modifiers.shift);
    let hover_pos = pointer.hover_pos().filter(|p| rect.contains(*p));

    // Esc cancels a drag in progress (the rest of the Esc ladder lives in the app).
    if app.drag.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        if let Some(d) = app.drag.as_mut() {
            d.cancelled = true;
        }
        app.preview = None;
        app.esc_consumed = true;
    }

    let gizmo_used = match app.tool {
        ActiveTool::Move | ActiveTool::Turn => gizmo(app, ui, rect, shift),
        _ => false,
    };
    let handles = if app.tool == ActiveTool::Size { size_handles(app, vp) } else { Vec::new() };
    if app.tool == ActiveTool::Size {
        size_interaction(app, ui, response, vp, &handles, shift);
        draw_size_handles(app, ui, &handles, hover_pos);
    }

    let over_gizmo = hover_pos.is_some_and(|p| app.gizmo.pick_preview((p.x, p.y))) || app.gizmo.is_focused();
    let over_handle = hover_pos.is_some_and(|p| handles.iter().any(|h| h.screen.distance(p) <= HANDLE_RADIUS + 3.0));

    // Hover highlight.
    if app.drag.is_none() && !over_gizmo && !over_handle {
        app.view.hover = hover_pos.and_then(|p| {
            let ray = app.camera.ray_through(vp, to_gvec(p));
            let doc = &app.doc;
            ss_doc::pick_object(doc, &mut app.eval, &ray, |id| !doc.is_locked(id)).map(|h| h.object)
        });
    } else if app.drag.is_some() {
        app.view.hover = None;
    }

    // Click to pick.
    if response.clicked_by(egui::PointerButton::Primary) && !gizmo_used && !over_gizmo && !over_handle {
        let hit = response.interact_pointer_pos().and_then(|p| {
            let ray = app.camera.ray_through(vp, to_gvec(p));
            let doc = &app.doc;
            ss_doc::pick_object(doc, &mut app.eval, &ray, |id| !doc.is_locked(id)).map(|h| h.object)
        });
        let mut sel = app.sel.clone();
        match (hit, shift) {
            (Some(id), true) => sel.toggle(id),
            (Some(id), false) => sel.set_only(top_level_pick(&app.doc, id, &app.sel)),
            (None, false) => sel.clear(),
            (None, true) => {}
        }
        app.set_selection(sel);
    }

    // Box select: drag on empty space with the Select tool.
    if app.tool == ActiveTool::Select && app.drag.is_none() && response.drag_started_by(egui::PointerButton::Primary) {
        if let Some(start) = pointer.press_origin().filter(|p| rect.contains(*p)) {
            let ray = app.camera.ray_through(vp, to_gvec(start));
            let doc = &app.doc;
            let on_object = ss_doc::pick_object(doc, &mut app.eval, &ray, |id| !doc.is_locked(id)).is_some();
            if !on_object && !over_gizmo {
                app.drag = Some(DragSession {
                    kind: DragKind::BoxSelect { start, additive: shift },
                    start_doc: app.doc.clone(),
                    cancelled: false,
                });
            }
        }
    }
    if let Some(DragSession { kind: DragKind::BoxSelect { start, additive }, cancelled, .. }) = app.drag.as_ref() {
        let (start, additive, cancelled) = (*start, *additive, *cancelled);
        let current = pointer.hover_pos().unwrap_or(start);
        let r = Rect::from_two_pos(start, current);
        if !cancelled {
            ui.painter().rect(
                r,
                2.0,
                theme::ACCENT.linear_multiply(0.12),
                Stroke::new(1.5, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        if !pointer.primary_down() {
            if !cancelled && r.width() > 3.0 && r.height() > 3.0 {
                let mut sel =
                    if additive { app.sel.clone() } else { Selection { filter: app.sel.filter, ..Default::default() } };
                for id in app.doc.walk() {
                    let obj_ok = app.doc.object(id).is_some_and(|o| o.shape().is_some())
                        && app.doc.is_visible(id)
                        && !app.doc.is_locked(id);
                    if !obj_ok {
                        continue;
                    }
                    let center = app.doc.world_bounds(id).center();
                    if app.camera.project(vp, center).is_some_and(|s| r.contains(Pos2::new(s.x, s.y))) {
                        sel.objects.insert(top_level_pick(&app.doc, id, &app.sel));
                    }
                }
                app.set_selection(sel);
            }
            app.drag = None;
        }
    }
}

/// Clicking a shape inside a group picks the group (like Roblox models), unless
/// the group is already picked, in which case it drills down to the shape.
fn top_level_pick(doc: &Document, id: ObjectId, sel: &Selection) -> ObjectId {
    let ancestors = doc.ancestors(id);
    if ancestors.iter().any(|a| sel.contains(*a)) {
        return id;
    }
    ancestors.last().copied().unwrap_or(id)
}

fn to_dvec(v: Vec3) -> DVec3 {
    v.as_dvec3()
}

/// The Move/Turn gizmo. Returns true if the gizmo handled the pointer this frame.
fn gizmo(app: &mut ShapeshopApp, ui: &mut egui::Ui, rect: Rect, shift: bool) -> bool {
    let base = match &app.drag {
        Some(DragSession { kind: DragKind::Gizmo { .. }, start_doc, .. }) => start_doc.clone(),
        Some(_) => return false,
        None => app.doc.clone(),
    };
    let ids = match &app.drag {
        Some(DragSession { kind: DragKind::Gizmo { ids, .. }, .. }) => ids.clone(),
        _ => manipulable(&base, &app.sel),
    };
    if ids.is_empty() {
        return false;
    }
    let turn = app.tool == ActiveTool::Turn;
    let vp = crate::viewport::egui_to_viewport(rect);
    let aspect = vp.aspect();
    let view: DMat4 = app.camera.view_matrix().as_dmat4();
    let proj: DMat4 = app.camera.projection_matrix(aspect).as_dmat4();
    let snap = base.meta().snap.clone();
    let config = GizmoConfig {
        view_matrix: view.into(),
        projection_matrix: proj.into(),
        viewport: rect,
        modes: if turn {
            GizmoMode::RotateX | GizmoMode::RotateY | GizmoMode::RotateZ
        } else {
            GizmoMode::TranslateX
                | GizmoMode::TranslateY
                | GizmoMode::TranslateZ
                | GizmoMode::TranslateXZ
                | GizmoMode::TranslateXY
                | GizmoMode::TranslateYZ
        },
        orientation: if app.local_axes { GizmoOrientation::Local } else { GizmoOrientation::Global },
        pivot_point: if turn {
            transform_gizmo_egui::config::TransformPivotPoint::IndividualOrigins
        } else {
            transform_gizmo_egui::config::TransformPivotPoint::MedianPoint
        },
        snapping: snap.enabled && !shift,
        snap_angle: snap.turn_deg.to_radians(),
        snap_distance: snap.move_step,
        visuals: GizmoVisuals {
            x_color: theme::AXIS_X,
            y_color: theme::AXIS_Y,
            z_color: theme::AXIS_Z,
            s_color: Color32::WHITE,
            inactive_alpha: 0.85,
            highlight_alpha: 1.0,
            highlight_color: Some(Color32::from_rgb(255, 214, 10)),
            stroke_width: 5.0,
            gizmo_size: 110.0,
        },
        ..Default::default()
    };
    app.gizmo.update_config(config);
    // The gizmo adds this frame's change to whatever we pass in, so pass the
    // transforms as currently previewed.
    let current = app.preview.as_ref().filter(|_| app.drag.is_some()).unwrap_or(&base);
    let targets: Vec<transform_gizmo_egui::math::Transform> = ids
        .iter()
        .map(|id| {
            let t = current.world_transform(*id);
            transform_gizmo_egui::math::Transform::from_scale_rotation_translation(
                DVec3::ONE,
                t.rotation.as_dquat(),
                to_dvec(t.translation),
            )
        })
        .collect();
    let result = app.gizmo.interact(ui, &targets);
    let mut used = false;
    if let Some((_res, new)) = result {
        used = true;
        if app.drag.is_none() {
            app.drag = Some(DragSession {
                kind: DragKind::Gizmo { ids: ids.clone(), turn },
                start_doc: base.clone(),
                cancelled: false,
            });
        }
        let cancelled = app.drag.as_ref().is_some_and(|d| d.cancelled);
        if !cancelled {
            let worlds: Vec<Transform> = new
                .iter()
                .map(|t| {
                    let r: DQuat = t.rotation.into();
                    let p: DVec3 = t.translation.into();
                    Transform::new(p.as_vec3(), r.as_quat().normalize())
                })
                .collect();
            let mut preview = base.clone();
            {
                let mut tx = preview.begin("preview");
                set_world_transforms(&mut tx, &ids, &worlds);
                tx.commit();
            }
            app.preview = Some(preview);
        }
    }
    // Released: commit the preview as one step.
    let released = !ui.input(|i| i.pointer.primary_down());
    if released && matches!(app.drag, Some(DragSession { kind: DragKind::Gizmo { .. }, .. })) {
        let drag = app.drag.take().expect("checked");
        let preview = app.preview.take();
        if let (false, Some(preview), DragKind::Gizmo { ids, turn }) = (drag.cancelled, preview, drag.kind) {
            let worlds: Vec<Transform> = ids.iter().map(|id| preview.world_transform(*id)).collect();
            let label = if turn { "Turn" } else { "Move" };
            app.commit_with(label, None, |tx| {
                set_world_transforms(tx, &ids, &worlds);
                Ok(())
            });
        }
    }
    used || app.gizmo.is_focused()
}

/// Put objects at the given world transforms (converting to their parents' space).
pub fn set_world_transforms(tx: &mut Tx<'_>, ids: &[ObjectId], worlds: &[Transform]) {
    for (id, world) in ids.iter().zip(worlds) {
        let parent = tx.doc().parent_world_transform(*id);
        if let Ok(obj) = tx.object_mut(*id) {
            obj.transform = parent.inverse().mul(world);
        }
    }
}

/// The six handles on the active object's sides (only for unlocked shapes).
fn size_handles(app: &mut ShapeshopApp, vp: &Viewport) -> Vec<SizeHandle> {
    let doc = app.preview.as_ref().unwrap_or(&app.doc);
    let Some(id) = app.sel.active() else { return vec![] };
    if doc.is_locked(id) || doc.object(id).and_then(|o| o.shape()).is_none() {
        return vec![];
    }
    let Some(mesh) = app.eval.shape_mesh(doc, id) else { return vec![] };
    let b = mesh.bounds();
    if b.is_empty() {
        return vec![];
    }
    let world = doc.world_transform(id);
    let mut out = Vec::with_capacity(6);
    for axis in 0..3 {
        for side in [1.0f32, -1.0] {
            let mut local = b.center();
            local[axis] = if side > 0.0 { b.max[axis] } else { b.min[axis] };
            let w = world.transform_point(local);
            let dir = world.rotation * (Vec3::AXES[axis] * side);
            // Push the handle a little outside the face so it is easy to grab.
            let offset = app.camera.world_per_pixel_at(vp, w) * 18.0;
            let pos = w + dir * offset;
            if let Some(s) = app.camera.project(vp, pos) {
                out.push(SizeHandle { axis, side, world: pos, screen: Pos2::new(s.x, s.y), dir });
            }
        }
    }
    out
}

fn size_interaction(
    app: &mut ShapeshopApp,
    ui: &egui::Ui,
    response: &egui::Response,
    vp: &Viewport,
    handles: &[SizeHandle],
    shift: bool,
) {
    let pointer = ui.input(|i| i.pointer.clone());
    // Start a drag on a handle.
    if app.drag.is_none() && response.drag_started_by(egui::PointerButton::Primary) {
        let start = pointer.press_origin().unwrap_or_default();
        if let Some(h) = handles
            .iter()
            .filter(|h| h.screen.distance(start) <= HANDLE_RADIUS + 3.0)
            .min_by(|a, b| a.screen.distance(start).total_cmp(&b.screen.distance(start)))
        {
            let Some(id) = app.sel.active() else { return };
            let Some(mesh) = app.eval.shape_mesh(&app.doc, id) else { return };
            let line = Ray::new(h.world, h.dir);
            let ray = app.camera.ray_through(vp, to_gvec(start));
            let grab_t = line.closest_params_to_line(&ray).map(|(t, _)| t).unwrap_or(0.0);
            let len = mesh.bounds().size()[h.axis];
            app.drag = Some(DragSession {
                kind: DragKind::Size(SizeDrag {
                    id,
                    axis: h.axis,
                    side: h.side,
                    line,
                    grab_t,
                    start_len: len,
                    new_len: len,
                }),
                start_doc: app.doc.clone(),
                cancelled: false,
            });
        }
    }
    let Some(DragSession { kind: DragKind::Size(sd), start_doc, cancelled }) = app.drag.as_mut() else { return };
    if !*cancelled {
        if let Some(p) = pointer.hover_pos() {
            let ray = app.camera.ray_through(vp, to_gvec(p));
            let t = sd.line.closest_params_to_line(&ray).map(|(t, _)| t).unwrap_or(sd.grab_t);
            let snap = start_doc.meta().snap.clone();
            let mut len = sd.start_len + (t - sd.grab_t);
            if snap.enabled && !shift {
                len = ss_math::snap::snap(len, snap.size_step);
            }
            let min = start_doc.meta().min_part_size();
            sd.new_len = len.max(min);
            let (id, axis, side, new_len, start_len) = (sd.id, sd.axis, sd.side, sd.new_len, sd.start_len);
            let mut preview = start_doc.clone();
            {
                let mut tx = preview.begin("preview");
                let _ = resize_side(&mut tx, id, axis, side, new_len / start_len.max(1e-6));
                tx.commit();
            }
            app.preview = Some(preview);
        }
    }
    if !pointer.primary_down() {
        let drag = app.drag.take().expect("checked");
        app.preview = None;
        if let (false, DragKind::Size(sd)) = (drag.cancelled, drag.kind)
            && (sd.new_len - sd.start_len).abs() > 1e-6
        {
            let k = sd.new_len / sd.start_len.max(1e-6);
            app.commit_with("Size", None, |tx| resize_side(tx, sd.id, sd.axis, sd.side, k));
        }
    }
}

/// Scale a shape along one local axis by `k`, keeping the opposite side where it is
/// (Roblox-style one-sided resize). Primitives keep their sliders: their size and
/// pivot scale; edited shapes scale their points around the pivot.
pub fn resize_side(tx: &mut Tx<'_>, id: ObjectId, axis: usize, side: f32, k: f32) -> Result<(), ss_doc::DocError> {
    resize_axis(tx, id, axis, k, ResizeAnchor::OppositeOf(side))
}

/// What stays put when a shape is resized along one axis.
#[derive(Clone, Copy, Debug)]
pub enum ResizeAnchor {
    /// The side opposite the dragged one (+1 = dragging the + side).
    OppositeOf(f32),
    Center,
}

/// Scale a shape along one local axis by `k`, keeping `anchor` where it is.
pub fn resize_axis(
    tx: &mut Tx<'_>,
    id: ObjectId,
    axis: usize,
    k: f32,
    anchor: ResizeAnchor,
) -> Result<(), ss_doc::DocError> {
    let Some(mesh) = tx.doc().mesh_of(id) else { return Err(ss_doc::DocError::NotAShape(id)) };
    let b = mesh.bounds();
    // The anchor, in object space, before and after scaling about the origin.
    let a = match anchor {
        ResizeAnchor::OppositeOf(side) if side > 0.0 => b.min[axis],
        ResizeAnchor::OppositeOf(_) => b.max[axis],
        ResizeAnchor::Center => b.center()[axis],
    };
    let shift = a - a * k;
    let obj = tx.object_mut(id)?;
    let rotation = obj.transform.rotation;
    let shape = obj.shape_mut().ok_or(ss_doc::DocError::NotAShape(id))?;
    let mut scale = Vec3::ONE;
    scale[axis] = k;
    shape.pivot *= scale;
    match &mut shape.source {
        ShapeSource::Primitive(p) => {
            let mut size = p.size();
            size[axis] *= k;
            p.set_size(size);
        }
        ShapeSource::Mesh(_) => {
            let pivot = shape.pivot / scale; // the old pivot
            let m = glam::Mat4::from_translation(pivot * scale)
                * glam::Mat4::from_scale(scale)
                * glam::Mat4::from_translation(-pivot);
            let mesh = tx.mesh_mut(id)?;
            mesh.transform(&m);
        }
    }
    let mut delta = Vec3::ZERO;
    delta[axis] = shift;
    let obj = tx.object_mut(id)?;
    obj.transform.translation += Mat3::from_quat(rotation) * delta;
    Ok(())
}

fn draw_size_handles(app: &ShapeshopApp, ui: &egui::Ui, handles: &[SizeHandle], hover: Option<Pos2>) {
    let painter = ui.painter();
    let active_drag = match &app.drag {
        Some(DragSession { kind: DragKind::Size(sd), .. }) => Some(sd.clone()),
        _ => None,
    };
    for h in handles {
        let color = [theme::AXIS_X, theme::AXIS_Y, theme::AXIS_Z][h.axis];
        let hot = hover.is_some_and(|p| h.screen.distance(p) <= HANDLE_RADIUS + 3.0)
            || active_drag.as_ref().is_some_and(|d| d.axis == h.axis && d.side == h.side);
        let r = if hot { HANDLE_RADIUS + 2.0 } else { HANDLE_RADIUS };
        painter.circle(h.screen, r, color, Stroke::new(2.0, Color32::WHITE));
    }
    if let Some(sd) = active_drag
        && let Some(h) = handles.iter().find(|h| h.axis == sd.axis && h.side == sd.side)
    {
        let text = app.doc.meta().units.format(sd.new_len);
        let pos = h.screen + egui::vec2(16.0, -16.0);
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(15.0), Color32::WHITE);
        let bg = Rect::from_min_size(pos, galley.size()).expand(5.0);
        painter.rect_filled(bg, 6.0, Color32::from_rgba_unmultiplied(20, 24, 32, 220));
        painter.galley(pos, galley, Color32::WHITE);
    }
}
