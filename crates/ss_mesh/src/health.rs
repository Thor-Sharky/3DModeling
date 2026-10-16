//! "Is this shape closed and solid?" — the checks behind the badge, the export
//! pre-flight and the booleans' input validation.

use serde::Serialize;
use ss_math::newell_normal;

use crate::{FaceId, PolyMesh};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MeshHealth {
    pub verts: usize,
    pub edges: usize,
    pub faces: usize,
    /// Edges with only one face: the shape has a gap.
    pub boundary_edges: usize,
    /// Edges shared by three or more faces.
    pub nonmanifold_edges: usize,
    /// Neighboring faces that disagree about which way is out.
    pub winding_conflicts: usize,
    /// Faces whose points are no longer flat.
    pub bent_faces: Vec<FaceId>,
    /// Faces with (almost) no area.
    pub degenerate_faces: Vec<FaceId>,
    pub signed_volume: f64,
}

impl MeshHealth {
    pub fn compute(mesh: &PolyMesh) -> Self {
        let topo = mesh.topology();
        let size = mesh.bounds().size().max_element().max(1e-3);
        let flat_tol = (size * 1e-4).max(1e-5);
        let area_tol = (size * size * 1e-9).max(1e-12);
        let mut bent = Vec::new();
        let mut degenerate = Vec::new();
        for (fid, _) in mesh.faces() {
            let pts = mesh.face_positions(fid).unwrap_or_default();
            let nv = newell_normal(pts.iter().copied());
            let area = nv.length() * 0.5;
            if area <= area_tol {
                degenerate.push(fid);
                continue;
            }
            if pts.len() > 3 {
                let n = nv / (2.0 * area);
                let c = pts.iter().copied().sum::<glam::Vec3>() / pts.len() as f32;
                if pts.iter().any(|p| (*p - c).dot(n).abs() > flat_tol) {
                    bent.push(fid);
                }
            }
        }
        MeshHealth {
            verts: mesh.vert_count(),
            edges: topo.edge_count(),
            faces: mesh.face_count(),
            boundary_edges: topo.boundary_edges().len(),
            nonmanifold_edges: topo.nonmanifold_edges().len(),
            winding_conflicts: topo.winding_conflicts().len(),
            bent_faces: bent,
            degenerate_faces: degenerate,
            signed_volume: mesh.signed_volume(),
        }
    }

    /// Closed, consistently wound, outward-facing, with no zero-area faces.
    pub fn is_solid(&self) -> bool {
        self.faces > 0
            && self.boundary_edges == 0
            && self.nonmanifold_edges == 0
            && self.winding_conflicts == 0
            && self.degenerate_faces.is_empty()
            && self.signed_volume > 0.0
    }

    /// V - E + F (2 for a closed shape without holes through it, 0 for a ring).
    pub fn euler_characteristic(&self) -> i64 {
        self.verts as i64 - self.edges as i64 + self.faces as i64
    }

    /// One friendly sentence describing the most important problem, if any.
    pub fn problem_summary(&self) -> Option<String> {
        if self.faces == 0 {
            return Some("This shape has no faces.".into());
        }
        if self.boundary_edges > 0 {
            return Some("This shape has a gap, so it isn't closed and solid.".into());
        }
        if self.nonmanifold_edges > 0 {
            return Some("Some edges are shared by more than two faces.".into());
        }
        if self.winding_conflicts > 0 || self.signed_volume <= 0.0 {
            return Some("Some faces are inside-out.".into());
        }
        if !self.degenerate_faces.is_empty() {
            return Some("Some faces are squashed flat to nothing.".into());
        }
        if !self.bent_faces.is_empty() {
            return Some("Some faces are bent (their points aren't flat).".into());
        }
        None
    }
}
