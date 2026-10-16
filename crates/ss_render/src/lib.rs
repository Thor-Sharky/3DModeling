//! OpenGL 3.3 core renderer for the viewport, thumbnails and "Take Picture".
//!
//! It consumes a [`RenderScene`] (plain data built by `ss_doc`) and draws into
//! its own framebuffer. The viewport texture keeps the same GL name across
//! resizes, so the app registers it with egui once.
//!
//! Only `eframe::glow` is used (never a direct `glow` dependency, which would
//! create a second, incompatible context type).

pub mod gl_util;
pub mod shaders;

use std::collections::HashMap;
use std::sync::Arc;

use eframe::glow::{self, HasContext as _};
use glam::{Mat4, Vec3};
use ss_doc::{DrawStyle, Highlight, ObjectId, RenderScene, ViewStyle};
use ss_mesh::render::{edge_segments, linear_to_srgb};
use ss_mesh::{RenderMesh, Shading};

use gl_util::Program;
pub use gl_util::{GlInfo, RenderTarget};

const FLOATS_PER_VERTEX: usize = 12;
const MAX_LIGHTS: usize = 8;

struct GpuMesh {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    index_count: i32,
    last_used: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MeshKey {
    object: ObjectId,
    mesh_rev: u64,
    /// The object's own material: faces without one take this color.
    material: ss_mesh::MaterialId,
    /// Hash of the whole palette (editing a swatch recolors everything).
    materials: u64,
    shading: u32,
}

fn shading_key(s: Shading) -> u32 {
    match s {
        Shading::Faceted => 0,
        Shading::Smooth => 1,
        Shading::SmoothSharp { angle_deg } => 2 + angle_deg.to_bits(),
    }
}

/// Options for one render.
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    /// Leave the background transparent (pictures with a see-through background).
    pub transparent_background: bool,
    pub draw_grid: bool,
    pub draw_outlines: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { transparent_background: false, draw_grid: true, draw_outlines: true }
    }
}

pub struct GlRenderer {
    gl: Arc<glow::Context>,
    shaded: Program,
    background: Program,
    grid: Program,
    mask: Program,
    outline: Program,
    overlay: Program,
    empty_vao: glow::VertexArray,
    grid_vao: glow::VertexArray,
    grid_vbo: glow::Buffer,
    overlay_vao: glow::VertexArray,
    overlay_vbo: glow::Buffer,
    meshes: HashMap<MeshKey, GpuMesh>,
    frame: u64,
    msaa: i32,
    viewport: Option<RenderTarget>,
    info: GlInfo,
}

impl GlRenderer {
    /// `msaa` = requested samples (0 = off); clamped to what the GPU supports.
    pub fn new(gl: Arc<glow::Context>, msaa: u32) -> Result<Self, String> {
        let info = GlInfo::query(&gl);
        let (major, minor) = info.version_number();
        if (major, minor) < (3, 3) {
            return Err(format!(
                "Shapeshop needs OpenGL 3.3, but this computer only offers OpenGL {major}.{minor} ({}).",
                info.renderer
            ));
        }
        let shaded = Program::new(&gl, shaders::SHADED_VS, &shaders::with_common(shaders::SHADED_FS))?;
        let background = Program::new(&gl, shaders::FULLSCREEN_VS, shaders::BACKGROUND_FS)?;
        let grid = Program::new(&gl, shaders::GRID_VS, shaders::GRID_FS)?;
        let mask = Program::new(&gl, shaders::MASK_VS, shaders::MASK_FS)?;
        let outline = Program::new(&gl, shaders::FULLSCREEN_VS, shaders::OUTLINE_FS)?;
        let overlay = Program::new(&gl, shaders::OVERLAY_VS, shaders::OVERLAY_FS)?;
        unsafe {
            let empty_vao = gl.create_vertex_array()?;
            let grid_vao = gl.create_vertex_array()?;
            let grid_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(grid_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(grid_vbo));
            let quad: [f32; 12] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&quad), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            let overlay_vao = gl.create_vertex_array()?;
            let overlay_vbo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(overlay_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(overlay_vbo));
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 28, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 4, glow::FLOAT, false, 28, 12);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            let max = info.max_samples.max(0);
            Ok(Self {
                gl,
                shaded,
                background,
                grid,
                mask,
                outline,
                overlay,
                empty_vao,
                grid_vao,
                grid_vbo,
                overlay_vao,
                overlay_vbo,
                meshes: HashMap::new(),
                frame: 0,
                msaa: (msaa as i32).min(max),
                viewport: None,
                info,
            })
        }
    }

    pub fn info(&self) -> &GlInfo {
        &self.info
    }

    pub fn gl(&self) -> &Arc<glow::Context> {
        &self.gl
    }

    /// Render the viewport. Returns the texture holding the image, whose GL name
    /// stays the same for the renderer's lifetime (register it with egui once).
    pub fn render_viewport(&mut self, scene: &RenderScene, width: u32, height: u32) -> Result<glow::Texture, String> {
        let mut target = match self.viewport.take() {
            Some(t) => t,
            None => RenderTarget::new(&self.gl, width as i32, height as i32, self.msaa)?,
        };
        let result = target.resize(&self.gl, width as i32, height as i32).map(|_| {
            self.render(scene, &target, RenderOptions::default());
            target.color_tex
        });
        self.viewport = Some(target);
        result
    }

    /// Render offscreen at any size and read back RGBA8 rows (top first).
    pub fn render_image(
        &mut self,
        scene: &RenderScene,
        width: u32,
        height: u32,
        opts: RenderOptions,
    ) -> Result<Vec<u8>, String> {
        let samples = if self.msaa > 0 { self.msaa.max(4).min(self.info.max_samples.max(0)) } else { 0 };
        let target = RenderTarget::new(&self.gl, width as i32, height as i32, samples)?;
        self.render(scene, &target, opts);
        let pixels = target.read_rgba(&self.gl);
        target.destroy(&self.gl, false);
        Ok(pixels)
    }

    fn render(&mut self, scene: &RenderScene, target: &RenderTarget, opts: RenderOptions) {
        self.frame += 1;
        let gl = self.gl.clone();
        let aspect = target.width as f32 / target.height as f32;
        let view_proj = scene.camera.view_proj(aspect);
        let eye = scene.camera.eye();
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::FRAMEBUFFER_SRGB);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.draw_fbo()));
            gl.viewport(0, 0, target.width, target.height);
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear_depth_f32(1.0);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT);

            // 1. Background
            if !opts.transparent_background {
                let (top, bottom) = match scene.style {
                    ViewStyle::RenderPreview => (to_srgb3(scene.env.sky_top), to_srgb3(scene.env.sky_bottom)),
                    _ => ([0.66, 0.74, 0.84], [0.90, 0.92, 0.95]),
                };
                gl.disable(glow::DEPTH_TEST);
                gl.disable(glow::BLEND);
                gl.use_program(Some(self.background.raw));
                let l = self.background.loc(&gl, "u_top");
                gl.uniform_3_f32_slice(l.as_ref(), &top);
                let l = self.background.loc(&gl, "u_bottom");
                gl.uniform_3_f32_slice(l.as_ref(), &bottom);
                gl.bind_vertex_array(Some(self.empty_vao));
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }

            // 2. Opaque shapes
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.front_face(glow::CCW);
            gl.disable(glow::BLEND);
            self.setup_shaded(scene, view_proj, eye);
            for item in scene.items.iter().filter(|i| i.style == DrawStyle::Normal && !scene.xray) {
                self.draw_item(
                    scene,
                    item,
                    0,
                    if item.selected {
                        0.06
                    } else if item.hovered {
                        0.12
                    } else {
                        0.0
                    },
                );
            }

            // 3. Face highlights (element picking) over the opaque pass
            self.draw_face_highlights(scene, view_proj);

            // 4. Grid (blended, depth-tested, no depth writes)
            if let (Some(grid), true) = (scene.grid, opts.draw_grid) {
                gl.enable(glow::BLEND);
                gl.blend_func_separate(
                    glow::SRC_ALPHA,
                    glow::ONE_MINUS_SRC_ALPHA,
                    glow::ONE,
                    glow::ONE_MINUS_SRC_ALPHA,
                );
                gl.depth_mask(false);
                gl.disable(glow::CULL_FACE);
                gl.use_program(Some(self.grid.raw));
                let center = Vec3::new(scene.camera.focus.x, 0.0, scene.camera.focus.z);
                let p = &mut self.grid;
                let l = p.loc(&gl, "u_view_proj");
                gl.uniform_matrix_4_f32_slice(l.as_ref(), false, &view_proj.to_cols_array());
                let l = p.loc(&gl, "u_center");
                gl.uniform_3_f32_slice(l.as_ref(), &center.to_array());
                let l = p.loc(&gl, "u_extent");
                gl.uniform_1_f32(l.as_ref(), grid.extent);
                let l = p.loc(&gl, "u_step");
                gl.uniform_1_f32(l.as_ref(), grid.step.max(1e-3));
                let l = p.loc(&gl, "u_major");
                gl.uniform_1_f32(l.as_ref(), grid.major_every.max(1) as f32);
                let l = p.loc(&gl, "u_camera_pos");
                gl.uniform_3_f32_slice(l.as_ref(), &eye.to_array());
                gl.bind_vertex_array(Some(self.grid_vao));
                gl.draw_arrays(glow::TRIANGLES, 0, 6);
                gl.depth_mask(true);
            }

            // 5. Translucent: cutters, ghosts, x-ray
            gl.enable(glow::BLEND);
            gl.blend_func_separate(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.depth_mask(false);
            gl.disable(glow::CULL_FACE);
            self.setup_shaded(scene, view_proj, eye);
            for item in &scene.items {
                let mode = match (item.style, scene.xray) {
                    (DrawStyle::Cutter, _) => 1,
                    (DrawStyle::Ghost, _) => 2,
                    (DrawStyle::Normal, true) => 3,
                    (DrawStyle::Normal, false) => continue,
                };
                self.draw_item(scene, item, mode, if item.selected { 0.15 } else { 0.0 });
            }
            gl.depth_mask(true);

            // 6. Edges and points for element picking
            self.draw_edges_and_points(scene, view_proj, target);

            target.resolve(&gl);

            // 7. Selection / hover outline (screen-space, drawn on the resolved image)
            if opts.draw_outlines && scene.items.iter().any(|i| i.selected || i.hovered) {
                self.draw_outlines(scene, view_proj, target);
            }

            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.depth_mask(true);
        }
        self.evict_unused();
    }

    unsafe fn setup_shaded(&mut self, scene: &RenderScene, view_proj: Mat4, eye: Vec3) {
        let gl = &self.gl;
        let p = &mut self.shaded;
        let env = &scene.env;
        unsafe {
            gl.use_program(Some(p.raw));
            let l = p.loc(gl, "u_view_proj");
            gl.uniform_matrix_4_f32_slice(l.as_ref(), false, &view_proj.to_cols_array());
            let l = p.loc(gl, "u_camera_pos");
            gl.uniform_3_f32_slice(l.as_ref(), &eye.to_array());
            let l = p.loc(gl, "u_sun_dir");
            gl.uniform_3_f32_slice(l.as_ref(), &env.sun_dir.normalize_or(Vec3::Y).to_array());
            let l = p.loc(gl, "u_sun_color");
            gl.uniform_3_f32_slice(l.as_ref(), &env.sun_color);
            let l = p.loc(gl, "u_sky_top");
            gl.uniform_3_f32_slice(l.as_ref(), &env.sky_top);
            let l = p.loc(gl, "u_sky_bottom");
            gl.uniform_3_f32_slice(l.as_ref(), &env.sky_bottom);
            let l = p.loc(gl, "u_ambient");
            gl.uniform_1_f32(l.as_ref(), env.ambient * 2.0);
            let l = p.loc(gl, "u_exposure");
            gl.uniform_1_f32(l.as_ref(), 1.0);
            let n = scene.lights.len().min(MAX_LIGHTS);
            let mut pos = [0f32; MAX_LIGHTS * 3];
            let mut col = [0f32; MAX_LIGHTS * 3];
            let mut dir = [0f32; MAX_LIGHTS * 3];
            let mut range = [0f32; MAX_LIGHTS];
            let mut cos = [-1f32; MAX_LIGHTS];
            for (i, light) in scene.lights.iter().take(n).enumerate() {
                pos[i * 3..i * 3 + 3].copy_from_slice(&light.position.to_array());
                col[i * 3..i * 3 + 3].copy_from_slice(&light.color);
                dir[i * 3..i * 3 + 3].copy_from_slice(&light.direction.to_array());
                range[i] = light.range;
                cos[i] = light.cos_cutoff;
            }
            let l = p.loc(gl, "u_num_lights");
            gl.uniform_1_i32(l.as_ref(), n as i32);
            let l = p.loc(gl, "u_light_pos");
            gl.uniform_3_f32_slice(l.as_ref(), &pos);
            let l = p.loc(gl, "u_light_color");
            gl.uniform_3_f32_slice(l.as_ref(), &col);
            let l = p.loc(gl, "u_light_dir");
            gl.uniform_3_f32_slice(l.as_ref(), &dir);
            let l = p.loc(gl, "u_light_range");
            gl.uniform_1_f32_slice(l.as_ref(), &range);
            let l = p.loc(gl, "u_light_cos");
            gl.uniform_1_f32_slice(l.as_ref(), &cos);
        }
    }

    unsafe fn draw_item(&mut self, scene: &RenderScene, item: &ss_doc::DrawItem, mode: i32, tint: f32) {
        let key = MeshKey {
            object: item.object,
            mesh_rev: item.mesh.rev(),
            material: item.material,
            materials: scene.materials_key(),
            shading: shading_key(item.shading),
        };
        let frame = self.frame;
        if !self.meshes.contains_key(&key) {
            let rm = RenderMesh::build(&item.mesh, item.shading, |fm| scene.vertex_material(item.material, fm));
            match unsafe { upload_mesh(&self.gl, &rm) } {
                Ok(m) => {
                    self.meshes.insert(key, m);
                }
                Err(e) => {
                    log::error!("mesh upload failed: {e}");
                    return;
                }
            }
        }
        let gl = self.gl.clone();
        let p = &mut self.shaded;
        let mesh = self.meshes.get_mut(&key).expect("inserted above");
        mesh.last_used = frame;
        unsafe {
            let l = p.loc(&gl, "u_model");
            gl.uniform_matrix_4_f32_slice(l.as_ref(), false, &item.world.to_cols_array());
            let l = p.loc(&gl, "u_mode");
            gl.uniform_1_i32(l.as_ref(), mode);
            let l = p.loc(&gl, "u_tint");
            gl.uniform_1_f32(l.as_ref(), tint);
            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
        }
    }

    unsafe fn draw_face_highlights(&mut self, scene: &RenderScene, view_proj: Mat4) {
        let mut verts: Vec<f32> = Vec::new();
        let mut draws: Vec<(Mat4, std::ops::Range<i32>)> = Vec::new();
        for item in &scene.items {
            if item.faces.is_empty() {
                continue;
            }
            let start = (verts.len() / 7) as i32;
            let tri = item.mesh.triangulation();
            for (t, f) in tri.tris.iter().zip(&tri.tri_face) {
                let Some((_, h)) = item.faces.iter().find(|(id, _)| id == f) else { continue };
                let color = match h {
                    Highlight::Selected => [0.0, 0.635, 1.0, 0.45],
                    Highlight::Hover => [1.0, 1.0, 1.0, 0.22],
                };
                for v in t {
                    let p = item.mesh.vert(*v).unwrap_or_default();
                    verts.extend_from_slice(&p.to_array());
                    verts.extend_from_slice(&color);
                }
            }
            draws.push((item.world, start..(verts.len() / 7) as i32));
        }
        if verts.is_empty() {
            return;
        }
        let gl = self.gl.clone();
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func_separate(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.enable(glow::POLYGON_OFFSET_FILL);
            gl.polygon_offset(-1.0, -1.0);
            gl.depth_mask(false);
            self.draw_overlay_batch(view_proj, &verts, &draws, glow::TRIANGLES);
            gl.depth_mask(true);
            gl.disable(glow::POLYGON_OFFSET_FILL);
            gl.disable(glow::BLEND);
        }
    }

    unsafe fn draw_edges_and_points(&mut self, scene: &RenderScene, view_proj: Mat4, target: &RenderTarget) {
        let aspect = target.width as f32 / target.height as f32;
        let mut verts: Vec<f32> = Vec::new();
        let mut draws: Vec<(Mat4, std::ops::Range<i32>)> = Vec::new();
        let px = 2.0 / target.height as f32; // NDC units per pixel (vertical)
        for item in scene.items.iter().filter(|i| i.show_edges || i.show_points) {
            let start = (verts.len() / 7) as i32;
            let mvp = view_proj * item.world;
            let to_clip = |p: Vec3| mvp * p.extend(1.0);
            // Screen-space quads built in clip space, then sent through an identity model.
            let mut push_quad = |a: glam::Vec4, b: glam::Vec4, width_px: f32, color: [f32; 4]| {
                if a.w <= 1e-4 || b.w <= 1e-4 {
                    return;
                }
                let na = a.truncate() / a.w;
                let nb = b.truncate() / b.w;
                let mut dir = glam::Vec2::new((nb.x - na.x) * aspect, nb.y - na.y);
                if dir.length_squared() < 1e-12 {
                    dir = glam::Vec2::X;
                }
                let n = glam::Vec2::new(-dir.y, dir.x).normalize() * width_px * 0.5 * px;
                let off = glam::Vec2::new(n.x / aspect, n.y);
                let bias = -0.0008;
                let corners = [
                    (na.truncate() + off, na.z + bias),
                    (na.truncate() - off, na.z + bias),
                    (nb.truncate() - off, nb.z + bias),
                    (nb.truncate() + off, nb.z + bias),
                ];
                for i in [0, 1, 2, 0, 2, 3] {
                    let (xy, z) = corners[i];
                    verts.extend_from_slice(&[xy.x, xy.y, z]);
                    verts.extend_from_slice(&color);
                }
            };
            if item.show_edges {
                for (e, [a, b]) in edge_segments(&item.mesh) {
                    let h = item.edges.iter().find(|(k, _)| *k == e).map(|(_, h)| *h);
                    let (w, c) = match h {
                        Some(Highlight::Selected) => (3.5, [0.0, 0.635, 1.0, 1.0]),
                        Some(Highlight::Hover) => (3.0, [1.0, 1.0, 1.0, 0.95]),
                        None => (1.5, [0.08, 0.10, 0.14, 0.75]),
                    };
                    push_quad(to_clip(a), to_clip(b), w, c);
                }
            }
            if item.show_points {
                for (v, p) in item.mesh.verts() {
                    let h = item.points.iter().find(|(k, _)| *k == v).map(|(_, h)| *h);
                    let (s, c) = match h {
                        Some(Highlight::Selected) => (8.0, [0.0, 0.635, 1.0, 1.0]),
                        Some(Highlight::Hover) => (8.0, [1.0, 1.0, 1.0, 1.0]),
                        None => (6.0, [0.08, 0.10, 0.14, 0.95]),
                    };
                    let c0 = to_clip(p);
                    if c0.w <= 1e-4 {
                        continue;
                    }
                    // A short horizontal segment as wide as it is long makes a square.
                    let half = s * 0.5 * px * c0.w / aspect;
                    let a = c0 - glam::Vec4::new(half, 0.0, 0.0, 0.0);
                    let b = c0 + glam::Vec4::new(half, 0.0, 0.0, 0.0);
                    push_quad(a, b, s, c);
                }
            }
            draws.push((Mat4::IDENTITY, start..(verts.len() / 7) as i32));
        }
        if verts.is_empty() {
            return;
        }
        let gl = self.gl.clone();
        unsafe {
            gl.enable(glow::BLEND);
            gl.blend_func_separate(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::CULL_FACE);
            gl.depth_mask(false);
            if scene.xray {
                gl.disable(glow::DEPTH_TEST);
            }
            self.draw_overlay_batch(Mat4::IDENTITY, &verts, &draws, glow::TRIANGLES);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_mask(true);
            gl.disable(glow::BLEND);
        }
    }

    unsafe fn draw_overlay_batch(
        &mut self,
        view_proj: Mat4,
        verts: &[f32],
        draws: &[(Mat4, std::ops::Range<i32>)],
        mode: u32,
    ) {
        let gl = &self.gl;
        let p = &mut self.overlay;
        unsafe {
            gl.use_program(Some(p.raw));
            gl.bind_vertex_array(Some(self.overlay_vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.overlay_vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(verts), glow::STREAM_DRAW);
            let lvp = p.loc(gl, "u_view_proj");
            gl.uniform_matrix_4_f32_slice(lvp.as_ref(), false, &view_proj.to_cols_array());
            let lm = p.loc(gl, "u_model");
            for (model, range) in draws {
                gl.uniform_matrix_4_f32_slice(lm.as_ref(), false, &model.to_cols_array());
                gl.draw_arrays(mode, range.start, range.end - range.start);
            }
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
    }

    unsafe fn draw_outlines(&mut self, scene: &RenderScene, view_proj: Mat4, target: &RenderTarget) {
        let gl = self.gl.clone();
        unsafe {
            // Mask: 1.0 for selected, 0.5 for hovered.
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.mask_fbo));
            gl.viewport(0, 0, target.width, target.height);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::MAX);
            gl.blend_func(glow::ONE, glow::ONE);
            gl.use_program(Some(self.mask.raw));
            let l = self.mask.loc(&gl, "u_view_proj");
            gl.uniform_matrix_4_f32_slice(l.as_ref(), false, &view_proj.to_cols_array());
            for item in scene.items.iter().filter(|i| i.selected || i.hovered) {
                let key = MeshKey {
                    object: item.object,
                    mesh_rev: item.mesh.rev(),
                    material: item.material,
                    materials: scene.materials_key(),
                    shading: shading_key(item.shading),
                };
                let Some(mesh) = self.meshes.get(&key) else { continue };
                let l = self.mask.loc(&gl, "u_model");
                gl.uniform_matrix_4_f32_slice(l.as_ref(), false, &item.world.to_cols_array());
                let l = self.mask.loc(&gl, "u_value");
                gl.uniform_1_f32(l.as_ref(), if item.selected { 1.0 } else { 0.5 });
                gl.bind_vertex_array(Some(mesh.vao));
                gl.draw_elements(glow::TRIANGLES, mesh.index_count, glow::UNSIGNED_INT, 0);
            }
            gl.blend_equation(glow::FUNC_ADD);

            // Composite onto the resolved image.
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.resolve_fbo));
            gl.viewport(0, 0, target.width, target.height);
            gl.blend_func_separate(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.outline.raw));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(target.mask_tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            let l = self.outline.loc(&gl, "u_mask");
            gl.uniform_1_i32(l.as_ref(), 0);
            let l = self.outline.loc(&gl, "u_texel");
            gl.uniform_2_f32(l.as_ref(), 1.0 / target.width as f32, 1.0 / target.height as f32);
            let l = self.outline.loc(&gl, "u_radius");
            gl.uniform_1_f32(l.as_ref(), 2.5);
            gl.bind_vertex_array(Some(self.empty_vao));
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.disable(glow::BLEND);
        }
    }

    fn evict_unused(&mut self) {
        let frame = self.frame;
        let gl = &self.gl;
        self.meshes.retain(|_, m| {
            let keep = frame.saturating_sub(m.last_used) < 120;
            if !keep {
                unsafe {
                    gl.delete_vertex_array(m.vao);
                    gl.delete_buffer(m.vbo);
                    gl.delete_buffer(m.ebo);
                }
            }
            keep
        });
    }

    /// Free GPU resources (call from `App::on_exit`).
    pub fn destroy(&mut self) {
        let gl = self.gl.clone();
        unsafe {
            for (_, m) in self.meshes.drain() {
                gl.delete_vertex_array(m.vao);
                gl.delete_buffer(m.vbo);
                gl.delete_buffer(m.ebo);
            }
            gl.delete_vertex_array(self.empty_vao);
            gl.delete_vertex_array(self.grid_vao);
            gl.delete_buffer(self.grid_vbo);
            gl.delete_vertex_array(self.overlay_vao);
            gl.delete_buffer(self.overlay_vbo);
        }
        for p in [&self.shaded, &self.background, &self.grid, &self.mask, &self.outline, &self.overlay] {
            p.destroy(&gl);
        }
        if let Some(t) = self.viewport.take() {
            // egui owns the registered texture.
            t.destroy(&gl, true);
        }
    }
}

unsafe fn upload_mesh(gl: &glow::Context, rm: &RenderMesh) -> Result<GpuMesh, String> {
    let mut data = Vec::with_capacity(rm.positions.len() * FLOATS_PER_VERTEX);
    for i in 0..rm.positions.len() {
        data.extend_from_slice(&rm.positions[i]);
        data.extend_from_slice(&rm.normals[i]);
        data.extend_from_slice(&rm.colors[i]);
        data.extend_from_slice(&rm.surface[i]);
    }
    unsafe {
        let vao = gl.create_vertex_array()?;
        let vbo = gl.create_buffer()?;
        let ebo = gl.create_buffer()?;
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck::cast_slice(&data), glow::STATIC_DRAW);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
        gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytemuck::cast_slice(&rm.indices), glow::STATIC_DRAW);
        let stride = (FLOATS_PER_VERTEX * 4) as i32;
        for (loc, offset) in [(0u32, 0i32), (1, 12), (2, 24), (3, 36)] {
            gl.enable_vertex_attrib_array(loc);
            gl.vertex_attrib_pointer_f32(loc, 3, glow::FLOAT, false, stride, offset);
        }
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
        Ok(GpuMesh { vao, vbo, ebo, index_count: rm.indices.len() as i32, last_used: 0 })
    }
}

fn to_srgb3(c: [f32; 3]) -> [f32; 3] {
    c.map(linear_to_srgb)
}
