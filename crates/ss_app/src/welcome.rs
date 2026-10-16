//! The Welcome card (first run and File > New) and the Settings window.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use ss_doc::{DocMeta, ProjectTarget};
use ss_ops::AppAction;

use crate::app::ShapeshopApp;
use crate::theme::{self, ThemeChoice};
use crate::viewport::ControlsPreset;

#[derive(Clone, Debug, Default)]
pub struct WelcomeState {
    pub open: bool,
    pub target: Option<ProjectTarget>,
    pub device: Option<ControlsPreset>,
}

pub fn show_welcome(app: &mut ShapeshopApp, ctx: &egui::Context) {
    if !app.welcome.open {
        return;
    }
    let mut start = false;
    egui::Modal::new(egui::Id::new("welcome")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(RichText::new(format!("{}  Welcome to Shapeshop!", icon::HAND_WAVING)).size(24.0));
        ui.label("Two quick questions so everything is set up the right way for you.");
        ui.add_space(10.0);

        ui.label(RichText::new("What are you making?").strong());
        ui.horizontal(|ui| {
            for (t, ic, tip) in [
                (ProjectTarget::Roblox, icon::CUBE, "Sizes in studs, ready to import into Roblox Studio."),
                (
                    ProjectTarget::GameAsset,
                    icon::GAME_CONTROLLER,
                    "For Unity, Godot and other game engines. Sizes in meters.",
                ),
                (ProjectTarget::ArtScene, icon::PAINT_BRUSH, "Make a scene with lights and take nice pictures."),
            ] {
                let picked = app.welcome.target == Some(t);
                let b = egui::Button::new(RichText::new(format!("{ic}\n{}", t.label())).size(16.0))
                    .selected(picked)
                    .min_size(egui::vec2(170.0, 64.0));
                let r = ui.add(b).on_hover_text(tip);
                app.anchors.register(format!("welcome.target.{t:?}").to_lowercase(), r.rect);
                if r.clicked() {
                    app.welcome.target = Some(t);
                }
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("How do you move around?").strong());
        ui.horizontal(|ui| {
            for (d, label, tip) in [
                (
                    ControlsPreset::RobloxStudio,
                    "Mouse with a wheel",
                    "Right-drag to look around, middle-drag to slide, scroll to zoom (like Roblox Studio).",
                ),
                (
                    ControlsPreset::Trackpad,
                    "Laptop trackpad",
                    "Two-finger scroll to look around, Shift + scroll to slide, pinch to zoom.",
                ),
                (
                    ControlsPreset::Blender,
                    "Blender controls",
                    "Middle-drag to look around, Shift + middle-drag to slide.",
                ),
            ] {
                let picked = app.welcome.device == Some(d);
                let r = ui
                    .add(egui::Button::new(label).selected(picked).min_size(egui::vec2(170.0, 34.0)))
                    .on_hover_text(tip);
                app.anchors.register(format!("welcome.device.{d:?}").to_lowercase(), r.rect);
                if r.clicked() {
                    app.welcome.device = Some(d);
                }
            }
        });
        ui.label(RichText::new("Not sure? Pick Mouse. You can change both later in Settings.").weak().small());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let ready = app.welcome.target.is_some();
            let r = ui
                .add_enabled(
                    ready,
                    egui::Button::new(RichText::new("Start building").strong().color(egui::Color32::WHITE))
                        .fill(theme::ACCENT),
                )
                .on_disabled_hover_text("Pick what you're making first.");
            app.anchors.register("welcome.start", r.rect);
            if r.clicked() {
                start = true;
            }
        });
    });
    if start {
        let target = app.welcome.target.unwrap_or_default();
        let device = app.welcome.device.unwrap_or_default();
        app.prefs.controls = device;
        app.welcome.open = false;
        app.new_project(DocMeta::for_target(target));
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsState {
    pub open: bool,
}

pub fn show_settings(app: &mut ShapeshopApp, ctx: &egui::Context) {
    if !app.settings.open {
        return;
    }
    let mut open = true;
    let mut actions = Vec::new();
    egui::Window::new(format!("{}  Settings", icon::GEAR))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            egui::Grid::new("settings").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Controls");
                ui.horizontal(|ui| {
                    for c in [ControlsPreset::RobloxStudio, ControlsPreset::Trackpad, ControlsPreset::Blender] {
                        if ui.selectable_label(app.prefs.controls == c, c.label()).clicked() {
                            actions.push(AppAction::SetSetting { key: "controls".into(), value: c.key().into() });
                        }
                    }
                });
                ui.end_row();
                ui.label("Theme");
                ui.horizontal(|ui| {
                    for (t, label) in [(ThemeChoice::Light, "Light"), (ThemeChoice::Dark, "Dark")] {
                        if ui.selectable_label(app.prefs.theme == t, label).clicked() {
                            actions.push(AppAction::SetSetting { key: "theme".into(), value: label.to_lowercase() });
                        }
                    }
                });
                ui.end_row();
                ui.label("UI size");
                let mut scale = app.prefs.ui_scale;
                if ui.add(egui::Slider::new(&mut scale, 80..=200).suffix("%").step_by(5.0)).changed() {
                    actions.push(AppAction::SetSetting { key: "ui_scale".into(), value: scale.to_string() });
                }
                ui.end_row();
                ui.label("Reduce motion");
                let mut rm = app.prefs.reduce_motion;
                if ui.checkbox(&mut rm, "No celebrations or pulsing highlights").changed() {
                    actions.push(AppAction::SetSetting { key: "reduce_motion".into(), value: rm.to_string() });
                }
                ui.end_row();
                ui.label("Autosave every");
                let mut secs = app.prefs.autosave_secs / 60;
                if ui.add(egui::Slider::new(&mut secs, 1..=15).suffix(" min")).changed() {
                    actions.push(AppAction::SetSetting { key: "autosave_minutes".into(), value: secs.to_string() });
                }
                ui.end_row();
            });
        });
    app.settings.open = open;
    for a in actions {
        app.dispatch(a);
    }
}

/// Apply a setting by key (from Settings, the search box or quests).
pub fn apply_setting(app: &mut ShapeshopApp, ctx: &egui::Context, key: &str, value: &str) -> Result<(), String> {
    match key {
        "theme" => {
            app.prefs.theme = match value {
                "dark" => ThemeChoice::Dark,
                "light" => ThemeChoice::Light,
                _ => return Err(format!("unknown theme {value}")),
            };
            theme::apply(ctx, app.prefs.theme);
        }
        "ui_scale" => {
            let v: u32 = value.parse().map_err(|_| format!("bad UI size {value}"))?;
            app.prefs.ui_scale = v.clamp(80, 200);
            if !app.args.deterministic {
                ctx.set_zoom_factor(app.prefs.ui_scale as f32 / 100.0);
            }
        }
        "controls" => {
            app.prefs.controls = ControlsPreset::from_key(value).ok_or_else(|| format!("unknown controls {value}"))?;
        }
        "reduce_motion" => app.prefs.reduce_motion = value == "true",
        "autosave_minutes" => {
            let m: u64 = value.parse().map_err(|_| format!("bad minutes {value}"))?;
            app.prefs.autosave_secs = m.clamp(1, 60) * 60;
        }
        "local_axes" => app.local_axes = if value == "toggle" { !app.local_axes } else { value == "true" },
        _ => return Err(format!("unknown setting {key}")),
    }
    Ok(())
}
