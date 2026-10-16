//! Palette swatches. A swatch *is* a material: editing it recolors everything using it.

use serde::{Deserialize, Serialize};
use ss_mesh::VertexMaterial;
use ss_mesh::render::srgb_to_linear;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// sRGB, 0..1 (what the color picker shows).
    pub color: [f32; 3],
    /// 0 = glossy, 1 = matte.
    pub roughness: f32,
    /// 0 = plastic, 1 = metal.
    pub metal: f32,
    /// Light the surface gives off (0 = none).
    pub glow: f32,
}

impl Material {
    pub fn new(name: impl Into<String>, color: [f32; 3]) -> Self {
        Self { name: name.into(), color, roughness: 0.6, metal: 0.0, glow: 0.0 }
    }

    pub fn from_hex(name: impl Into<String>, hex: u32) -> Self {
        Self::new(name, hex_to_rgb(hex))
    }

    pub fn with_surface(mut self, roughness: f32, metal: f32, glow: f32) -> Self {
        self.roughness = roughness;
        self.metal = metal;
        self.glow = glow;
        self
    }

    pub fn linear_color(&self) -> [f32; 3] {
        self.color.map(srgb_to_linear)
    }

    pub fn vertex_material(&self) -> VertexMaterial {
        VertexMaterial { color: self.linear_color(), roughness: self.roughness, metal: self.metal, glow: self.glow }
    }

    pub fn hex(&self) -> String {
        let [r, g, b] = self.color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

pub fn hex_to_rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 0xFF) as f32 / 255.0, ((hex >> 8) & 0xFF) as f32 / 255.0, (hex & 0xFF) as f32 / 255.0]
}

/// The starting palette for new projects (Roblox-flavored BrickColors).
pub fn default_palette() -> Vec<Material> {
    vec![
        Material::from_hex("Medium stone grey", 0xA3A2A5),
        Material::from_hex("White", 0xF2F3F3),
        Material::from_hex("Black", 0x1B2A35),
        Material::from_hex("Bright red", 0xC4281C),
        Material::from_hex("Bright orange", 0xDA8541),
        Material::from_hex("Bright yellow", 0xF5CD30),
        Material::from_hex("Bright green", 0x4B974B),
        Material::from_hex("Bright blue", 0x0D69AC),
        Material::from_hex("Really blue", 0x0000FF),
        Material::from_hex("Bright violet", 0x6B327C),
        Material::from_hex("Pink", 0xFF66CC),
        Material::from_hex("Reddish brown", 0x694028),
        Material::from_hex("Sand", 0xE6D2A4),
        Material::from_hex("Gold", 0xEFB838).with_surface(0.3, 1.0, 0.0),
        Material::from_hex("Metal", 0xC7C8CA).with_surface(0.35, 1.0, 0.0),
        Material::from_hex("Neon cyan", 0x04AFEC).with_surface(0.5, 0.0, 2.0),
    ]
}
