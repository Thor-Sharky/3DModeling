//! The Scene list: every object, with show/hide and lock toggles, rename and a right-click menu.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use ss_doc::{Combine, ObjectId, ObjectKind, ParamValue, ParamValues, Role};
use ss_ops::AppAction;

use crate::app::ShapeshopApp;
use crate::theme;

#[derive(Clone, Debug, Default)]
pub struct OutlinerState {
    pub renaming: Option<(ObjectId, String)>,
    /// Focus the rename box once, on the frame renaming starts.
    pub focus_rename: bool,
    pub collapsed: std::collections::BTreeSet<ObjectId>,
}

fn kind_icon(kind: &ObjectKind) -> (&'static str, Option<egui::Color32>) {
    match kind {
        ObjectKind::Shape(s) if s.role == Role::Cutter => (icon::SCISSORS, Some(theme::CUTTER_RED)),
        ObjectKind::Shape(_) => (icon::CUBE, None),
        ObjectKind::Group(g) if g.combine == Combine::None => (icon::FOLDER_SIMPLE, None),
        ObjectKind::Group(_) => (icon::INTERSECT, None),
        ObjectKind::Light(_) => (icon::LIGHTBULB, None),
        ObjectKind::Camera(_) => (icon::CAMERA, None),
    }
}

fn toggle(id: &str, value: bool) -> AppAction {
    let mut params = ParamValues::new();
    params.insert("value".into(), ParamValue::Bool(value));
    AppAction::RunTool { id: id.into(), params }
}

pub fn show(app: &mut ShapeshopApp, ui: &mut egui::Ui) {
    theme::section(ui, icon::TREE_VIEW, "SCENE");
    let mut actions: Vec<AppAction> = Vec::new();
    let mut pick: Option<(ObjectId, bool)> = None;
    let mut rename_done: Option<(ObjectId, String)> = None;
    egui::ScrollArea::vertical().id_salt("outliner").max_height(ui.available_height() * 0.42).show(ui, |ui| {
        ui.set_min_width(236.0);
        if app.doc.object_count() == 0 {
            ui.label(RichText::new("Your scene is empty. Click a shape on the left to add one.").weak());
        }
        let order = app.doc.walk();
        for id in order {
            // Skip rows inside collapsed groups.
            if app.doc.ancestors(id).iter().any(|a| app.outliner.collapsed.contains(a)) {
                continue;
            }
            let Some(obj) = app.doc.object(id) else { continue };
            let depth = app.doc.ancestors(id).len();
            let selected = app.sel.contains(id);
            let (ic, color) = kind_icon(&obj.kind);
            let has_children = !obj.children.is_empty();
            let (visible, locked, name) = (obj.visible, obj.locked, obj.name.clone());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.add_space(depth as f32 * 14.0);
                if has_children {
                    let collapsed = app.outliner.collapsed.contains(&id);
                    let arrow = if collapsed { icon::CARET_RIGHT } else { icon::CARET_DOWN };
                    if ui
                        .small_button(arrow)
                        .on_hover_text(if collapsed { "Show what's inside" } else { "Fold up" })
                        .clicked()
                    {
                        if collapsed {
                            app.outliner.collapsed.remove(&id);
                        } else {
                            app.outliner.collapsed.insert(id);
                        }
                    }
                }
                let renaming = app.outliner.renaming.as_ref().is_some_and(|(r, _)| *r == id);
                if renaming {
                    let (_, text) = app.outliner.renaming.as_mut().expect("checked");
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                    if std::mem::take(&mut app.outliner.focus_rename) {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        if !cancel {
                            rename_done = Some((id, text.clone()));
                        }
                        app.outliner.renaming = None;
                        app.esc_consumed = true;
                    }
                } else {
                    let mut text = RichText::new(format!("{ic}  {name}"));
                    if let Some(c) = color {
                        text = text.color(c);
                    }
                    if !visible {
                        text = text.weak().italics();
                    }
                    let r = ui.add(egui::Button::selectable(selected, text).min_size(egui::vec2(120.0, 0.0)));
                    app.anchors.register(format!("scene.{name}"), r.rect);
                    if r.double_clicked() {
                        app.outliner.renaming = Some((id, name.clone()));
                        app.outliner.focus_rename = true;
                    } else if r.clicked() {
                        pick = Some((id, ui.input(|i| i.modifiers.shift)));
                    }
                    r.context_menu(|ui| {
                        if !selected {
                            pick = Some((id, false));
                        }
                        if ui.button(format!("{} Rename", icon::PENCIL_SIMPLE)).clicked() {
                            app.outliner.renaming = Some((id, name.clone()));
                            app.outliner.focus_rename = true;
                            ui.close();
                        }
                        for (label, ic, tool) in [
                            ("Copy", icon::COPY, "object.duplicate"),
                            ("Delete", icon::TRASH, "object.delete"),
                            ("Group", icon::FOLDER_SIMPLE_PLUS, "object.group"),
                            ("Ungroup", icon::FOLDER_OPEN, "object.ungroup"),
                            ("Put on Ground", icon::ARROW_LINE_DOWN, "object.put_on_ground"),
                        ] {
                            if ui.button(format!("{ic} {label}")).clicked() {
                                actions.push(AppAction::run(tool));
                                ui.close();
                            }
                        }
                    });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let lock_ic = if locked { icon::LOCK_SIMPLE } else { icon::LOCK_SIMPLE_OPEN };
                    let r = ui.add(egui::Button::new(RichText::new(lock_ic).weak()).frame(false)).on_hover_text(
                        if locked {
                            "Locked: can't be picked in the 3D view. Click to unlock."
                        } else {
                            "Lock so it can't be picked by accident"
                        },
                    );
                    if r.clicked() {
                        actions.push(AppAction::Select(ss_ops::SelectOp::Only(id)));
                        actions.push(toggle("object.set_locked", !locked));
                    }
                    let eye = if visible { icon::EYE } else { icon::EYE_SLASH };
                    let r = ui
                        .add(egui::Button::new(RichText::new(eye).weak()).frame(false))
                        .on_hover_text(if visible { "Hide" } else { "Show" });
                    if r.clicked() {
                        actions.push(AppAction::Select(ss_ops::SelectOp::Only(id)));
                        actions.push(toggle("object.set_visible", !visible));
                    }
                });
            });
        }
    });
    if let Some((id, shift)) = pick {
        let mut sel = app.sel.clone();
        if shift {
            sel.toggle(id)
        } else {
            sel.set_only(id)
        }
        app.set_selection(sel);
    }
    if let Some((id, name)) = rename_done {
        let name = name.trim().to_owned();
        if !name.is_empty() && app.doc.object(id).is_some_and(|o| o.name != name) {
            app.dispatch(AppAction::Select(ss_ops::SelectOp::Only(id)));
            let mut params = ParamValues::new();
            params.insert("name".into(), ParamValue::Text(name));
            app.dispatch(AppAction::RunTool { id: "object.rename".into(), params });
        }
    }
    // Toggles act on one row without changing what is picked.
    let keep = app.sel.clone();
    let restore = actions.iter().any(|a| matches!(a, AppAction::Select(_)));
    for a in actions {
        app.dispatch(a);
    }
    if restore {
        app.set_selection(keep);
    }
}
