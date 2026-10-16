//! Right-side panels: Properties (exact numbers and shape sliders) and History.

use std::hash::{Hash, Hasher};

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use glam::{EulerRot, Quat, Vec3};
use ss_doc::{ObjectId, ObjectKind, Role, ShapeSource};
use ss_math::{Transform, Units};
use ss_mesh::{Primitive, Shading};
use ss_ops::AppAction;

use crate::app::ShapeshopApp;
use crate::gestures::{ResizeAnchor, resize_axis, set_world_transforms};
use crate::theme;

fn key(id: ObjectId, field: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    field.hash(&mut h);
    h.finish()
}

/// Three compact number boxes (X, Y, Z) with colored axis letters. Returns the new value if edited.
fn vec3_row(
    ui: &mut egui::Ui,
    label: &str,
    tip: &str,
    value: Vec3,
    speed: f32,
    suffix: &str,
    min: f32,
) -> Option<Vec3> {
    let mut v = value;
    let mut changed = false;
    ui.label(RichText::new(format!("{label}{suffix}")).small().weak()).on_hover_text(tip);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (i, (axis, color)) in
            [("X", theme::AXIS_X), ("Y", theme::AXIS_Y), ("Z", theme::AXIS_Z)].into_iter().enumerate()
        {
            ui.label(RichText::new(axis).color(color).strong());
            let r = ui.add_sized(
                [56.0, 22.0],
                egui::DragValue::new(&mut v[i]).speed(speed).range(min..=f32::MAX).max_decimals(2),
            );
            changed |= r.changed();
        }
    });
    (changed && v != value).then_some(v)
}

pub fn properties(app: &mut ShapeshopApp, ui: &mut egui::Ui) {
    theme::section(ui, icon::SLIDERS_HORIZONTAL, "PROPERTIES");
    ui.set_max_width(236.0);
    ui.spacing_mut().slider_width = 104.0;
    let Some(id) = app.sel.active() else {
        ui.label(RichText::new("Pick something to see its settings.").weak());
        return;
    };
    if app.sel.objects.len() > 1 {
        ui.label(RichText::new(format!("{} things picked. Settings show the last one.", app.sel.objects.len())).weak());
    }
    let doc = app.shown_doc().clone();
    let Some(obj) = doc.object(id) else { return };
    let units: Units = doc.meta().units;
    let suffix = format!("  ({})", units.short());

    // Name.
    let mut name = obj.name.clone();
    ui.horizontal(|ui| {
        ui.label("Name");
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(170.0));
        if r.lost_focus() && name.trim() != obj.name && !name.trim().is_empty() {
            let new = name.trim().to_owned();
            app.commit_with("Rename", None, |tx| {
                tx.object_mut(id)?.name = new;
                Ok(())
            });
        }
    });
    ui.label(RichText::new(obj.kind_label()).weak());

    // Position and turn (world space, at the pivot).
    let world = doc.world_transform(id);
    if let Some(p) = vec3_row(
        ui,
        "Position",
        "Where its pivot is. Type a number or drag left and right.",
        world.translation,
        0.05,
        &suffix,
        f32::MIN,
    ) {
        app.commit_coalesced("Move", key(id, "pos"), |tx| {
            set_world_transforms(tx, &[id], &[Transform::new(p, world.rotation)]);
            Ok(())
        });
    }
    let (ex, ey, ez) = world.rotation.to_euler(EulerRot::XYZ);
    // Round away float noise so the boxes never show "-0" or 89.99999.
    let euler = (Vec3::new(ex, ey, ez) * (180.0 / std::f32::consts::PI) * 1000.0).round() / 1000.0 + Vec3::ZERO;
    if let Some(e) = vec3_row(
        ui,
        "Turn",
        "How far it is turned around each direction, in degrees.",
        euler,
        1.0,
        "  (degrees)",
        f32::MIN,
    ) {
        let r = Quat::from_euler(EulerRot::XYZ, e.x.to_radians(), e.y.to_radians(), e.z.to_radians());
        app.commit_coalesced("Turn", key(id, "rot"), |tx| {
            set_world_transforms(tx, &[id], &[Transform::new(world.translation, r)]);
            Ok(())
        });
    }

    // Size and sliders for shapes.
    if let ObjectKind::Shape(shape) = &obj.kind {
        if let Some(mesh) = app.eval.shape_mesh(&doc, id) {
            let size = mesh.bounds().size();
            let min = doc.meta().min_part_size();
            if let Some(new) = vec3_row(ui, "Size", "How big it is along each direction.", size, 0.05, &suffix, min) {
                app.commit_coalesced("Size", key(id, "size"), |tx| {
                    for axis in 0..3 {
                        if (new[axis] - size[axis]).abs() > 1e-6 && size[axis] > 0.0 {
                            resize_axis(tx, id, axis, new[axis] / size[axis], ResizeAnchor::Center)?;
                        }
                    }
                    Ok(())
                });
            }
        }
        if let ShapeSource::Primitive(p) = &shape.source {
            primitive_sliders(app, ui, id, p);
        } else {
            ui.label(RichText::new("Locked in: its faces were edited, so the shape sliders are gone.").weak().small());
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Look").on_hover_text("Blocky faces, or smooth like a ball.");
            let mut look = shape.look;
            egui::ComboBox::from_id_salt("look").selected_text(look_label(look)).show_ui(ui, |ui| {
                for l in [Shading::Faceted, Shading::Smooth, Shading::SmoothSharp { angle_deg: 30.0 }] {
                    ui.selectable_value(&mut look, l, look_label(l));
                }
            });
            if look != shape.look {
                app.commit_with("Look", None, |tx| {
                    if let Some(s) = tx.object_mut(id)?.shape_mut() {
                        s.look = look;
                    }
                    Ok(())
                });
            }
        });
        ui.horizontal(|ui| {
            let mut cutter = shape.role == Role::Cutter;
            let r = ui
                .checkbox(&mut cutter, "Cutter")
                .on_hover_text("A cutter carves its shape out of solid parts it overlaps in a Union (Roblox: Negate).");
            if r.changed() {
                app.commit_with(if cutter { "Make Cutter" } else { "Make Solid" }, None, |tx| {
                    if let Some(s) = tx.object_mut(id)?.shape_mut() {
                        s.role = if cutter { Role::Cutter } else { Role::Solid };
                    }
                    Ok(())
                });
            }
            if let Some(m) = doc.material(shape.material) {
                let [r, g, b] = m.color.map(|c| (c * 255.0).round() as u8);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(r, g, b));
                ui.label(&m.name);
            }
        });
    }

    ui.horizontal(|ui| {
        let mut visible = obj.visible;
        if ui.checkbox(&mut visible, "Visible").on_hover_text("Hidden things are not shown and not exported.").changed()
        {
            app.commit_with(if visible { "Show" } else { "Hide" }, None, |tx| {
                tx.object_mut(id)?.visible = visible;
                Ok(())
            });
        }
        let mut locked = obj.locked;
        if ui.checkbox(&mut locked, "Locked").on_hover_text("Locked things can't be picked in the 3D view.").changed() {
            app.commit_with(if locked { "Lock" } else { "Unlock" }, None, |tx| {
                tx.object_mut(id)?.locked = locked;
                Ok(())
            });
        }
    });
}

fn look_label(l: Shading) -> &'static str {
    match l {
        Shading::Faceted => "Faceted",
        Shading::Smooth => "Smooth",
        Shading::SmoothSharp { .. } => "Smooth, sharp edges",
    }
}

/// Sliders that keep a shape editable ("live until edited").
fn primitive_sliders(app: &mut ShapeshopApp, ui: &mut egui::Ui, id: ObjectId, p: &Primitive) {
    let mut q = p.clone();
    let mut changed = false;
    fn slider(ui: &mut egui::Ui, label: &str, tip: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
        ui.horizontal(|ui| {
            ui.label(label).on_hover_text(tip);
            ui.add(egui::Slider::new(v, range).max_decimals(2)).changed()
        })
        .inner
    }
    fn int_slider(
        ui: &mut egui::Ui,
        label: &str,
        tip: &str,
        v: &mut u16,
        range: std::ops::RangeInclusive<u16>,
    ) -> bool {
        ui.horizontal(|ui| {
            ui.label(label).on_hover_text(tip);
            ui.add(egui::Slider::new(v, range)).changed()
        })
        .inner
    }
    match &mut q {
        Primitive::Block { roundness, .. } => {
            changed |= slider(ui, "Roundness", "Round off the corners and edges.", roundness, 0.0..=1.0);
        }
        Primitive::Cylinder { sides, roundness, .. } => {
            changed |= int_slider(ui, "Sides", "More sides look rounder but use more triangles.", sides, 3..=64);
            changed |= slider(ui, "Roundness", "Round off the top and bottom rims.", roundness, 0.0..=1.0);
        }
        Primitive::Ball { detail, .. } => {
            let mut d = *detail as u16;
            changed |= int_slider(ui, "Detail", "More detail looks rounder but uses more triangles.", &mut d, 0..=6);
            *detail = d as u8;
        }
        Primitive::Cone { sides, .. } | Primitive::Pyramid { sides, .. } => {
            changed |= int_slider(ui, "Sides", "How many sides the base has.", sides, 3..=64);
        }
        Primitive::Ring { thickness, sides, ring_sides, .. } => {
            changed |= slider(ui, "Thickness", "How thick the ring is.", thickness, 0.05..=10.0);
            changed |= int_slider(ui, "Sides", "How many steps around the ring.", sides, 3..=96);
            changed |= int_slider(ui, "Tube sides", "How round the ring's tube is.", ring_sides, 3..=32);
        }
        Primitive::Tube { thickness, sides, .. } => {
            changed |= slider(ui, "Thickness", "How thick the wall of the tube is.", thickness, 0.05..=10.0);
            changed |= int_slider(ui, "Sides", "More sides look rounder.", sides, 3..=64);
        }
        Primitive::Wedge { .. } | Primitive::CornerWedge { .. } => {}
    }
    if changed && q != *p {
        app.commit_coalesced("Change shape", key(id, "prim"), |tx| {
            if let Some(s) = tx.object_mut(id)?.shape_mut() {
                s.source = ShapeSource::Primitive(q);
            }
            Ok(())
        });
    }
}

/// The clickable History list: click a row to go back (or forward) to it.
pub fn history(app: &mut ShapeshopApp, ui: &mut egui::Ui) {
    theme::section(ui, icon::CLOCK_COUNTER_CLOCKWISE, "HISTORY");
    let cursor = app.history.cursor();
    let labels: Vec<String> = app.history.entries().iter().map(|e| e.label.clone()).collect();
    let mut jump = None;
    egui::ScrollArea::vertical().id_salt("history").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
        if ui
            .selectable_label(cursor == 0, RichText::new("Start").italics())
            .on_hover_text("Go back to the beginning")
            .clicked()
        {
            jump = Some(0);
        }
        for (i, label) in labels.iter().enumerate() {
            let applied = i < cursor;
            let text = if applied { RichText::new(label) } else { RichText::new(label).weak().strikethrough() };
            let r = ui.selectable_label(i + 1 == cursor, text).on_hover_text(if applied {
                "Click to go back to right after this step"
            } else {
                "Undone. Click to redo up to here"
            });
            if r.clicked() {
                jump = Some(i + 1);
            }
        }
    });
    if let Some(n) = jump {
        app.dispatch(AppAction::JumpHistory(n));
    }
}
