//! Look and feel: big, friendly targets and readable text.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Vec2};

/// Roblox Studio selection blue.
pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0xA2, 0xFF);
pub const CUTTER_RED: Color32 = Color32::from_rgb(0xDB, 0x29, 0x1F);
pub const OK_GREEN: Color32 = Color32::from_rgb(0x2E, 0x9E, 0x55);
pub const WARN_AMBER: Color32 = Color32::from_rgb(0xE0, 0x9A, 0x16);
pub const AXIS_X: Color32 = Color32::from_rgb(0xDB, 0x33, 0x2E);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x3E, 0xA8, 0x3B);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x26, 0x66, 0xE6);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ThemeChoice {
    #[default]
    Light,
    Dark,
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, theme: ThemeChoice) {
    let mut style = (*ctx.global_style()).clone();
    style.visuals = match theme {
        ThemeChoice::Light => light_visuals(),
        ThemeChoice::Dark => dark_visuals(),
    };
    style.text_styles = [
        (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.interact_size = Vec2::new(36.0, 26.0);
    style.spacing.slider_width = 140.0;
    style.spacing.combo_width = 140.0;
    style.interaction.tooltip_delay = 0.35;
    style.interaction.show_tooltips_only_when_still = false;
    ctx.set_global_style(style);
}

fn light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = Color32::from_rgb(0xF4, 0xF5, 0xF7);
    v.window_fill = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    v.extreme_bg_color = Color32::from_rgb(0xFF, 0xFF, 0xFF);
    v.faint_bg_color = Color32::from_rgb(0xEC, 0xEE, 0xF1);
    v.selection.bg_fill = ACCENT.linear_multiply(0.85);
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = Color32::from_rgb(0x00, 0x74, 0xC2);
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.widgets.open.corner_radius = r;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xE6, 0xE9, 0xED);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xD6, 0xEC, 0xFA);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.window_corner_radius = CornerRadius::same(10);
    v
}

fn dark_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.selection.bg_fill = ACCENT.linear_multiply(0.7);
    v.hyperlink_color = Color32::from_rgb(0x5C, 0xC3, 0xFF);
    let r = CornerRadius::same(6);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.window_corner_radius = CornerRadius::same(10);
    v
}

/// Section header used in the side panels.
pub fn section(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("{icon}  {title}")).strong().size(13.0).color(ui.visuals().weak_text_color()));
    ui.add_space(2.0);
}
