//! `--screenshot` support: ask eframe for a screenshot after N frames, save the
//! PNG when it arrives, then close the window.

use std::path::{Path, PathBuf};

use eframe::egui;

#[derive(Debug, Default)]
pub struct ScreenshotQueue {
    /// (path, frame to request on, quit after saving)
    pending: Vec<(PathBuf, u64, bool)>,
    requested: Option<(PathBuf, bool)>,
}

impl ScreenshotQueue {
    pub fn request(&mut self, path: PathBuf, at_frame: u64, quit_after: bool) {
        self.pending.push((path, at_frame, quit_after));
    }

    pub fn is_busy(&self) -> bool {
        !self.pending.is_empty() || self.requested.is_some()
    }

    /// Call once per frame. Returns `true` when the app should close.
    pub fn update(&mut self, ctx: &egui::Context, frame_no: u64) -> bool {
        let mut quit = false;
        // 1. A screenshot we asked for has arrived?
        if let Some((path, quit_after)) = self.requested.clone() {
            let image = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                match save_color_image(&path, &image) {
                    Ok(()) => log::info!("saved screenshot {}", path.display()),
                    Err(e) => log::error!("could not save screenshot {}: {e}", path.display()),
                }
                self.requested = None;
                quit = quit_after;
            } else {
                ctx.request_repaint();
            }
        }
        // 2. Time to ask for the next one?
        if self.requested.is_none()
            && let Some(pos) = self.pending.iter().position(|(_, at, _)| frame_no >= *at)
        {
            let (path, _, quit_after) = self.pending.remove(pos);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = Some((path, quit_after));
        }
        if self.is_busy() {
            ctx.request_repaint();
        }
        quit
    }
}

pub fn save_color_image(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8).map_err(|e| e.to_string())
}
