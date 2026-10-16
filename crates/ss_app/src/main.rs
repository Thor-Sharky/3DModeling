//! Shapeshop: an easy 3D modeling app for Roblox creators and hobby artists.

mod anchors;
mod app;
mod cli;
mod gestures;
mod panels;
mod screenshot;
mod script;
mod theme;
mod viewport;
mod welcome;

use eframe::egui;

fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,eframe=warn,egui_glow=warn"))
        .init();
    let args = match cli::parse() {
        Ok(a) => a,
        Err(msg) => {
            println!("{msg}");
            return if msg == cli::HELP { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::from(2) };
        }
    };

    if args.gl_info || args.self_test_gl {
        return gl_check(&args);
    }

    let size = args.size.unwrap_or([1600.0, 900.0]);
    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title(app::APP_NAME)
            .with_inner_size(size)
            .with_min_inner_size([960.0, 600.0]),
        depth_buffer: 0,
        multisampling: 0,
        ..Default::default()
    };
    if let Some(dir) = &args.data_dir {
        options.persistence_path = Some(dir.join("prefs.ron"));
    }
    let run_args = args.clone();
    let result = eframe::run_native(
        app::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(app::ShapeshopApp::new(cc, run_args)))),
    );
    match result {
        Ok(()) => {
            let code = app::EXIT_CODE.load(std::sync::atomic::Ordering::SeqCst);
            std::process::ExitCode::from(code as u8)
        }
        Err(e) => {
            let msg = format!(
                "Shapeshop couldn't open its window.\n\n{e}\n\nThis usually means the graphics driver is missing or too old. \
                 Updating your graphics driver often fixes it."
            );
            eprintln!("{msg}");
            if args.screenshot.is_none() && args.script.is_none() {
                let _ = rfd::MessageDialog::new()
                    .set_title("Shapeshop")
                    .set_description(&msg)
                    .set_level(rfd::MessageLevel::Error)
                    .show();
            }
            std::process::ExitCode::FAILURE
        }
    }
}

/// `--gl-info` / `--self-test-gl`: open a tiny window, query or exercise GL, quit.
fn gl_check(args: &cli::Args) -> std::process::ExitCode {
    use std::sync::{Arc, Mutex};
    let outcome: Arc<Mutex<Option<Result<String, String>>>> = Arc::default();
    let out = outcome.clone();
    let self_test = args.self_test_gl;
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 240.0]).with_visible(true),
        ..Default::default()
    };
    let r = eframe::run_native(
        "Shapeshop GL check",
        options,
        Box::new(move |cc| {
            let result = match cc.gl.clone() {
                None => Err("no OpenGL context".to_owned()),
                Some(gl) => {
                    let info = ss_render::GlInfo::query(&gl);
                    let text = format!(
                        "vendor: {}\nrenderer: {}\nversion: {}\nglsl: {}\nmax samples: {}",
                        info.vendor, info.renderer, info.version, info.glsl, info.max_samples
                    );
                    if self_test {
                        self_test_render(gl).map(|summary| format!("{text}\nself-test: {summary}"))
                    } else {
                        Ok(text)
                    }
                }
            };
            *out.lock().unwrap() = Some(result);
            Ok(Box::new(CloseImmediately))
        }),
    );
    if let Err(e) = r {
        eprintln!("could not create a window: {e}");
        return std::process::ExitCode::FAILURE;
    }
    match outcome.lock().unwrap().take() {
        Some(Ok(text)) => {
            println!("{text}");
            std::process::ExitCode::SUCCESS
        }
        Some(Err(e)) => {
            eprintln!("OpenGL check failed: {e}");
            std::process::ExitCode::FAILURE
        }
        None => std::process::ExitCode::FAILURE,
    }
}

/// Render a red block offscreen and check the center pixel is red-ish and the corner is background.
fn self_test_render(gl: std::sync::Arc<eframe::glow::Context>) -> Result<String, String> {
    use ss_doc::{Document, Evaluator, Object, ObjectKind, RenderScene, SceneOptions, Selection, Shape, ShapeSource};
    let mut renderer = ss_render::GlRenderer::new(gl, 0)?;
    let mut doc = Document::default();
    let red = doc.materials().find(|(_, m)| m.name == "Bright red").map(|(id, _)| id).ok_or("no red")?;
    let mut tx = doc.begin("test");
    let obj = Object::new(
        "Block 1",
        ObjectKind::Shape(Shape::new(
            ShapeSource::Primitive(ss_mesh::Primitive::Block { size: glam::Vec3::splat(4.0), roundness: 0.0, segs: 2 }),
            red,
        )),
    );
    tx.add_object(obj, None, None).map_err(|e| e.to_string())?;
    tx.commit();
    let mut camera = ss_math::OrbitCamera::home();
    camera.focus = glam::Vec3::ZERO;
    camera.distance = 12.0;
    let scene = RenderScene::build(
        &doc,
        &mut Evaluator::new(),
        &camera,
        &Selection::default(),
        &SceneOptions { show_grid: true, ..Default::default() },
    );
    let (w, h) = (128u32, 96u32);
    let px = renderer.render_image(&scene, w, h, ss_render::RenderOptions::default())?;
    renderer.destroy();
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let center = at(w / 2, h / 2);
    let corner = at(2, 2);
    let reddish = center[0] as i32 > center[1] as i32 + 40 && center[0] as i32 > center[2] as i32 + 40;
    if !reddish {
        return Err(format!("center pixel {center:?} is not red"));
    }
    // The background is a pale blue-grey gradient.
    if corner[3] != 255 || corner[2] <= corner[0] {
        return Err(format!("corner pixel {corner:?} doesn't look like background"));
    }
    Ok(format!("ok (center {center:?}, corner {corner:?})"))
}

struct CloseImmediately;

impl eframe::App for CloseImmediately {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
