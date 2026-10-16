//! A plain, testable description of what the viewport should draw. Built from
//! the document each frame; the GL renderer only consumes it.

use std::collections::BTreeMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};
use ss_math::OrbitCamera;
use ss_mesh::{EdgeKey, FaceId, MaterialId, PolyMesh, Shading, VertId, VertexMaterial};

use crate::object::ObjectKind;
use crate::selection::{PickFilter, Selection};
use crate::{Document, Evaluator, ObjectId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewStyle {
    /// Everything plain grey, to judge shape.
    Clay,
    #[default]
    Colors,
    /// Shadows and materials, like a picture.
    RenderPreview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawStyle {
    Normal,
    /// Red see-through stripes (cutters are never exported).
    Cutter,
    /// Translucent preview of a pending result.
    Ghost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Highlight {
    Hover,
    Selected,
}

#[derive(Clone, Debug)]
pub struct DrawItem {
    pub object: ObjectId,
    pub mesh: Arc<PolyMesh>,
    pub world: Mat4,
    pub material: MaterialId,
    pub shading: Shading,
    pub style: DrawStyle,
    pub selected: bool,
    pub hovered: bool,
    /// Tinted faces (element picking).
    pub faces: Vec<(FaceId, Highlight)>,
    /// Draw this object's edges (element picking shows them).
    pub show_edges: bool,
    pub edges: Vec<(EdgeKey, Highlight)>,
    pub show_points: bool,
    pub points: Vec<(VertId, Highlight)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSpec {
    pub step: f32,
    pub major_every: u32,
    pub extent: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Environment {
    /// Linear RGB.
    pub sky_top: [f32; 3],
    pub sky_bottom: [f32; 3],
    /// Direction *toward* the sun.
    pub sun_dir: Vec3,
    pub sun_color: [f32; 3],
    pub ambient: f32,
    pub shadows: bool,
    pub ground: bool,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            sky_top: [0.24, 0.45, 0.80],
            sky_bottom: [0.72, 0.79, 0.88],
            sun_dir: Vec3::new(0.45, 0.8, 0.35).normalize(),
            sun_color: [1.0, 0.97, 0.92],
            ambient: 0.35,
            shadows: false,
            ground: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderLight {
    pub position: Vec3,
    pub direction: Vec3,
    /// Linear RGB times brightness.
    pub color: [f32; 3],
    pub range: f32,
    /// Cosine of the half cone angle; -1 for a point light.
    pub cos_cutoff: f32,
}

#[derive(Clone, Debug)]
pub struct RenderScene {
    pub camera: OrbitCamera,
    pub items: Vec<DrawItem>,
    pub materials: BTreeMap<MaterialId, VertexMaterial>,
    pub style: ViewStyle,
    pub grid: Option<GridSpec>,
    pub env: Environment,
    pub lights: Vec<RenderLight>,
    pub xray: bool,
}

/// Inputs that are not part of the document.
#[derive(Clone, Debug, Default)]
pub struct SceneOptions {
    pub style: ViewStyle,
    pub show_grid: bool,
    pub xray: bool,
    pub hover: Option<ObjectId>,
    pub hover_face: Option<(ObjectId, FaceId)>,
    /// Objects hidden by "Focus on" (local view); `None` shows everything.
    pub focus_only: Option<Vec<ObjectId>>,
}

impl RenderScene {
    pub fn build(
        doc: &Document,
        eval: &mut Evaluator,
        camera: &OrbitCamera,
        sel: &Selection,
        opts: &SceneOptions,
    ) -> Self {
        let mut items = Vec::new();
        for id in doc.walk() {
            if !doc.is_visible(id) {
                continue;
            }
            if let Some(only) = &opts.focus_only
                && !only.contains(&id)
            {
                continue;
            }
            let Some(obj) = doc.object(id) else { continue };
            let ObjectKind::Shape(shape) = &obj.kind else { continue };
            let Some(mesh) = eval.shape_mesh(doc, id) else { continue };
            let element_mode = sel.filter != PickFilter::Objects && sel.elements.object == Some(id);
            let mut faces = Vec::new();
            let mut edges = Vec::new();
            let mut points = Vec::new();
            if element_mode {
                faces.extend(sel.elements.faces.iter().map(|f| (*f, Highlight::Selected)));
                edges.extend(sel.elements.edges.iter().map(|e| (*e, Highlight::Selected)));
                points.extend(sel.elements.verts.iter().map(|v| (*v, Highlight::Selected)));
            }
            if let Some((hid, hf)) = opts.hover_face
                && hid == id
                && !faces.iter().any(|(f, _)| *f == hf)
            {
                faces.push((hf, Highlight::Hover));
            }
            items.push(DrawItem {
                object: id,
                world: doc.world_transform(id).matrix(),
                material: shape.material,
                shading: shape.look,
                style: if shape.role == crate::Role::Cutter { DrawStyle::Cutter } else { DrawStyle::Normal },
                selected: sel.contains(id) && sel.filter == PickFilter::Objects,
                hovered: opts.hover == Some(id),
                faces,
                show_edges: element_mode,
                edges,
                show_points: element_mode && sel.filter == PickFilter::Points,
                points,
                mesh,
            });
        }
        let materials = doc.materials().map(|(id, m)| (id, m.vertex_material())).collect();
        let s = doc.scene();
        let srgb = |c: [f32; 3]| c.map(ss_mesh::render::srgb_to_linear);
        let env = Environment {
            sky_top: srgb(s.sky_top),
            sky_bottom: srgb(s.sky_bottom),
            sun_dir: sun_direction(s.time_of_day),
            sun_color: [s.sun_brightness, s.sun_brightness * 0.97, s.sun_brightness * 0.92],
            ambient: 0.35,
            shadows: s.shadows && opts.style == ViewStyle::RenderPreview,
            ground: s.ground && opts.style == ViewStyle::RenderPreview,
        };
        let lights = doc
            .walk()
            .into_iter()
            .filter(|id| doc.is_visible(*id))
            .filter_map(|id| {
                let ObjectKind::Light(l) = &doc.object(id)?.kind else { return None };
                let t = doc.world_transform(id);
                let lin = srgb(l.color).map(|c| c * l.brightness);
                Some(RenderLight {
                    position: t.translation,
                    direction: t.rotation * Vec3::NEG_Y,
                    color: lin,
                    range: l.range,
                    cos_cutoff: match l.kind {
                        crate::LightKind::Point => -1.0,
                        crate::LightKind::Spot { angle_deg } => (angle_deg * 0.5).to_radians().cos(),
                    },
                })
            })
            .take(8)
            .collect();
        RenderScene {
            camera: *camera,
            items,
            materials,
            style: opts.style,
            grid: opts.show_grid.then(|| GridSpec {
                step: doc.meta().grid_step,
                major_every: if doc.meta().units == ss_math::Units::Studs { 4 } else { 5 },
                extent: (camera.distance * 6.0).clamp(40.0, 2000.0),
            }),
            env,
            lights,
            xray: opts.xray,
        }
    }

    /// The material look for a face, honoring the Clay style.
    pub fn vertex_material(&self, item_material: MaterialId, face_material: Option<MaterialId>) -> VertexMaterial {
        if self.style == ViewStyle::Clay {
            return VertexMaterial { color: [0.62, 0.62, 0.64], roughness: 0.7, metal: 0.0, glow: 0.0 };
        }
        let id = face_material.unwrap_or(item_material);
        self.materials.get(&id).copied().unwrap_or_default()
    }

    /// Hash of everything that affects vertex colors (for GPU cache keys).
    pub fn materials_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.style.hash(&mut h);
        for (id, m) in &self.materials {
            id.hash(&mut h);
            for v in m.color.iter().chain([m.roughness, m.metal, m.glow].iter()) {
                v.to_bits().hash(&mut h);
            }
        }
        h.finish()
    }
}

/// Sun direction for a time of day (hours). Noon is high in the south-west sky.
pub fn sun_direction(hours: f32) -> Vec3 {
    let t = ((hours - 6.0) / 12.0).clamp(-0.1, 1.1); // 0 at sunrise, 1 at sunset
    let elevation = (t * std::f32::consts::PI).sin().max(0.05) * 1.2;
    let azimuth = std::f32::consts::PI * (0.25 + t);
    Vec3::new(azimuth.cos() * elevation.cos(), elevation.sin(), azimuth.sin() * elevation.cos()).normalize()
}
