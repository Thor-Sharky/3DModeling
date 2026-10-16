//! GPU-ready triangle data built from a mesh (pure CPU; the renderer only uploads it).

use std::collections::BTreeMap;

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::{EdgeKey, FaceId, MaterialId, PolyMesh, VertId};

/// How surfaces are shaded: "Looks" in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shading {
    /// Every face is flat (blocky / low-poly look).
    Faceted,
    /// Smooth everywhere.
    Smooth,
    /// Smooth, but edges sharper than `angle_deg` (and edges marked sharp) stay crisp.
    SmoothSharp { angle_deg: f32 },
}

impl Default for Shading {
    fn default() -> Self {
        Shading::Faceted
    }
}

/// Per-vertex material inputs for the shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VertexMaterial {
    /// Linear RGB.
    pub color: [f32; 3],
    pub roughness: f32,
    pub metal: f32,
    pub glow: f32,
}

impl Default for VertexMaterial {
    fn default() -> Self {
        Self { color: [0.8, 0.8, 0.8], roughness: 0.6, metal: 0.0, glow: 0.0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 3]>,
    /// (roughness, metal, glow)
    pub surface: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Face of each triangle (`indices.len() / 3` entries).
    pub tri_face: Vec<FaceId>,
}

impl RenderMesh {
    /// Build render data. `material` maps a face's material (or `None`) to its look.
    pub fn build(mesh: &PolyMesh, shading: Shading, material: impl Fn(Option<MaterialId>) -> VertexMaterial) -> Self {
        let tri = mesh.triangulation();
        let mut out = RenderMesh::default();
        let face_normals: BTreeMap<FaceId, Vec3> =
            mesh.faces().map(|(id, _)| (id, mesh.face_normal(id).unwrap_or(Vec3::Y))).collect();

        let smooth_normal = |face: FaceId, v: VertId| -> Vec3 {
            let fnorm = face_normals[&face];
            let threshold = match shading {
                Shading::Faceted => return fnorm,
                Shading::Smooth => -2.0,
                Shading::SmoothSharp { angle_deg } => angle_deg.to_radians().cos(),
            };
            let topo = mesh.topology();
            let mut sum = Vec3::ZERO;
            for f in topo.faces_of_vert(v) {
                let n = face_normals[f];
                let same_side_of_sharp = *f == face || !separated_by_sharp_edge(mesh, face, *f, v);
                if n.dot(fnorm) >= threshold && same_side_of_sharp {
                    sum += n * mesh.face_area(*f).unwrap_or(0.0).max(1e-8);
                }
            }
            sum.normalize_or(fnorm)
        };

        for (fid, range) in tri.face_ranges() {
            let Some(face) = mesh.face(fid) else { continue };
            let mat = material(face.material);
            // Vertices are per face corner so faces can differ in color and normal.
            let mut corner = BTreeMap::new();
            for t in &tri.tris[range.clone()] {
                for v in t {
                    let idx = *corner.entry(*v).or_insert_with(|| {
                        let p = mesh.vert(*v).unwrap_or_default();
                        let n = smooth_normal(fid, *v);
                        out.positions.push(p.to_array());
                        out.normals.push(n.to_array());
                        out.colors.push(mat.color);
                        out.surface.push([mat.roughness, mat.metal, mat.glow]);
                        (out.positions.len() - 1) as u32
                    });
                    out.indices.push(idx);
                }
                out.tri_face.push(fid);
            }
        }
        out
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// True if `a` and `b` touch at `v` only across an edge marked sharp.
fn separated_by_sharp_edge(mesh: &PolyMesh, a: crate::FaceId, b: crate::FaceId, v: VertId) -> bool {
    let (Some(ea), Some(eb)) = (mesh.face_edges(a), mesh.face_edges(b)) else { return false };
    let shared: Vec<EdgeKey> = ea.iter().filter(|e| e.contains(v) && eb.contains(e)).copied().collect();
    !shared.is_empty() && shared.iter().all(|e| mesh.edge_attrs(*e).sharp)
}

/// Unique edges as line segments (for edge overlays).
pub fn edge_segments(mesh: &PolyMesh) -> Vec<(EdgeKey, [Vec3; 2])> {
    mesh.edges().into_iter().filter_map(|e| Some((e, [mesh.vert(e.a)?, mesh.vert(e.b)?]))).collect()
}

/// sRGB (0..1) to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Primitive;

    #[test]
    fn faceted_block_has_24_corners() {
        let m = Primitive::default_block().build();
        let r = RenderMesh::build(&m, Shading::Faceted, |_| VertexMaterial::default());
        assert_eq!(r.positions.len(), 24);
        assert_eq!(r.triangle_count(), 12);
        assert_eq!(r.tri_face.len(), 12);
    }

    #[test]
    fn srgb_roundtrip() {
        for c in [0.0, 0.01, 0.2, 0.5, 0.9, 1.0] {
            assert!((linear_to_srgb(srgb_to_linear(c)) - c).abs() < 1e-5);
        }
    }
}
