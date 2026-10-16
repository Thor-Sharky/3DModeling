//! Small GL helpers: programs, offscreen targets, state save/restore.

use eframe::glow::{self, HasContext as _};

pub struct Program {
    pub raw: glow::Program,
    uniforms: std::collections::HashMap<&'static str, Option<glow::UniformLocation>>,
}

impl Program {
    pub fn new(gl: &glow::Context, vs: &str, fs: &str) -> Result<Self, String> {
        unsafe {
            let program = gl.create_program()?;
            let mut shaders = Vec::new();
            for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
                let s = gl.create_shader(kind)?;
                gl.shader_source(s, src);
                gl.compile_shader(s);
                if !gl.get_shader_compile_status(s) {
                    let log = gl.get_shader_info_log(s);
                    gl.delete_shader(s);
                    gl.delete_program(program);
                    return Err(format!("shader compile error: {log}"));
                }
                gl.attach_shader(program, s);
                shaders.push(s);
            }
            gl.link_program(program);
            for s in shaders {
                gl.detach_shader(program, s);
                gl.delete_shader(s);
            }
            if !gl.get_program_link_status(program) {
                let log = gl.get_program_info_log(program);
                gl.delete_program(program);
                return Err(format!("shader link error: {log}"));
            }
            Ok(Self { raw: program, uniforms: Default::default() })
        }
    }

    pub fn loc(&mut self, gl: &glow::Context, name: &'static str) -> Option<glow::UniformLocation> {
        let raw = self.raw;
        *self.uniforms.entry(name).or_insert_with(|| unsafe { gl.get_uniform_location(raw, name) })
    }

    pub fn destroy(&self, gl: &glow::Context) {
        unsafe { gl.delete_program(self.raw) };
    }
}

/// An offscreen color+depth target. With `samples > 0` it renders into
/// multisampled renderbuffers and resolves into `color_tex`.
pub struct RenderTarget {
    pub width: i32,
    pub height: i32,
    pub samples: i32,
    msaa_fbo: Option<glow::Framebuffer>,
    msaa_color: Option<glow::Renderbuffer>,
    msaa_depth: Option<glow::Renderbuffer>,
    pub resolve_fbo: glow::Framebuffer,
    resolve_depth: glow::Renderbuffer,
    /// The final color. Its GL name never changes across resizes, so it can be
    /// registered with egui once.
    pub color_tex: glow::Texture,
    /// Selection mask (R8), same size, no multisampling.
    pub mask_fbo: glow::Framebuffer,
    pub mask_tex: glow::Texture,
}

impl RenderTarget {
    pub fn new(gl: &glow::Context, width: i32, height: i32, samples: i32) -> Result<Self, String> {
        unsafe {
            let color_tex = gl.create_texture()?;
            let mask_tex = gl.create_texture()?;
            let resolve_fbo = gl.create_framebuffer()?;
            let resolve_depth = gl.create_renderbuffer()?;
            let mask_fbo = gl.create_framebuffer()?;
            let mut t = Self {
                width: 0,
                height: 0,
                samples,
                msaa_fbo: None,
                msaa_color: None,
                msaa_depth: None,
                resolve_fbo,
                resolve_depth,
                color_tex,
                mask_fbo,
                mask_tex,
            };
            t.resize(gl, width, height)?;
            Ok(t)
        }
    }

    pub fn resize(&mut self, gl: &glow::Context, width: i32, height: i32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return Ok(());
        }
        self.width = width;
        self.height = height;
        unsafe {
            for (tex, internal, format) in
                [(self.color_tex, glow::RGBA8, glow::RGBA), (self.mask_tex, glow::R8, glow::RED)]
            {
                gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    internal as i32,
                    width,
                    height,
                    0,
                    format,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(None),
                );
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);

            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(self.resolve_depth));
            gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH24_STENCIL8, width, height);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.resolve_fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(self.color_tex),
                0,
            );
            gl.framebuffer_renderbuffer(
                glow::FRAMEBUFFER,
                glow::DEPTH_STENCIL_ATTACHMENT,
                glow::RENDERBUFFER,
                Some(self.resolve_depth),
            );
            check_fbo(gl, "resolve")?;

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.mask_fbo));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(self.mask_tex),
                0,
            );
            check_fbo(gl, "mask")?;

            if self.samples > 0 {
                let fbo = match self.msaa_fbo {
                    Some(f) => f,
                    None => gl.create_framebuffer()?,
                };
                let color = match self.msaa_color {
                    Some(r) => r,
                    None => gl.create_renderbuffer()?,
                };
                let depth = match self.msaa_depth {
                    Some(r) => r,
                    None => gl.create_renderbuffer()?,
                };
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(color));
                gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, self.samples, glow::RGBA8, width, height);
                gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
                gl.renderbuffer_storage_multisample(
                    glow::RENDERBUFFER,
                    self.samples,
                    glow::DEPTH24_STENCIL8,
                    width,
                    height,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::RENDERBUFFER,
                    Some(color),
                );
                gl.framebuffer_renderbuffer(
                    glow::FRAMEBUFFER,
                    glow::DEPTH_STENCIL_ATTACHMENT,
                    glow::RENDERBUFFER,
                    Some(depth),
                );
                check_fbo(gl, "msaa")?;
                self.msaa_fbo = Some(fbo);
                self.msaa_color = Some(color);
                self.msaa_depth = Some(depth);
            }
            gl.bind_renderbuffer(glow::RENDERBUFFER, None);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        Ok(())
    }

    /// Where the scene is drawn.
    pub fn draw_fbo(&self) -> glow::Framebuffer {
        self.msaa_fbo.unwrap_or(self.resolve_fbo)
    }

    /// Copy the multisampled image (and depth) into `color_tex`. No-op without MSAA.
    pub fn resolve(&self, gl: &glow::Context) {
        if let Some(src) = self.msaa_fbo {
            unsafe {
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(src));
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.resolve_fbo));
                gl.blit_framebuffer(
                    0,
                    0,
                    self.width,
                    self.height,
                    0,
                    0,
                    self.width,
                    self.height,
                    glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT | glow::STENCIL_BUFFER_BIT,
                    glow::NEAREST,
                );
                gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            }
        }
    }

    /// Read the final color, top row first, as RGBA8.
    pub fn read_rgba(&self, gl: &glow::Context) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut pixels = vec![0u8; w * h * 4];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.resolve_fbo));
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        let mut flipped = Vec::with_capacity(pixels.len());
        for row in pixels.chunks_exact(w * 4).rev() {
            flipped.extend_from_slice(row);
        }
        flipped
    }

    /// Free everything except `color_tex` when it has been handed to egui.
    pub fn destroy(&self, gl: &glow::Context, keep_color_tex: bool) {
        unsafe {
            if let Some(f) = self.msaa_fbo {
                gl.delete_framebuffer(f);
            }
            if let Some(r) = self.msaa_color {
                gl.delete_renderbuffer(r);
            }
            if let Some(r) = self.msaa_depth {
                gl.delete_renderbuffer(r);
            }
            gl.delete_framebuffer(self.resolve_fbo);
            gl.delete_renderbuffer(self.resolve_depth);
            gl.delete_framebuffer(self.mask_fbo);
            gl.delete_texture(self.mask_tex);
            if !keep_color_tex {
                gl.delete_texture(self.color_tex);
            }
        }
    }
}

fn check_fbo(gl: &glow::Context, what: &str) -> Result<(), String> {
    let status = unsafe { gl.check_framebuffer_status(glow::FRAMEBUFFER) };
    if status == glow::FRAMEBUFFER_COMPLETE {
        Ok(())
    } else {
        Err(format!("{what} framebuffer incomplete: 0x{status:X}"))
    }
}

/// Information about the OpenGL driver (for `--gl-info` and bug reports).
#[derive(Clone, Debug, Default)]
pub struct GlInfo {
    pub vendor: String,
    pub renderer: String,
    pub version: String,
    pub glsl: String,
    pub max_samples: i32,
}

impl GlInfo {
    pub fn query(gl: &glow::Context) -> Self {
        unsafe {
            Self {
                vendor: gl.get_parameter_string(glow::VENDOR),
                renderer: gl.get_parameter_string(glow::RENDERER),
                version: gl.get_parameter_string(glow::VERSION),
                glsl: gl.get_parameter_string(glow::SHADING_LANGUAGE_VERSION),
                max_samples: gl.get_parameter_i32(glow::MAX_SAMPLES),
            }
        }
    }

    /// `(major, minor)` parsed from the version string.
    pub fn version_number(&self) -> (u32, u32) {
        let mut it = self.version.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
        let major = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        (major, minor)
    }
}
