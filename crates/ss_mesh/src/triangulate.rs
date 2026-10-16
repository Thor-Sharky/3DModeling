//! The single triangulation shared by drawing, picking, export and booleans.
//!
//! Triangles and quads are split directly (quads along the diagonal that keeps
//! both halves facing the same way and has the better shape). Larger or concave
//! faces go through earcut after projecting onto the face's best-fit plane.

use glam::{DVec3, Vec3};
use ss_math::newell_normal;

use crate::{FaceId, PolyMesh, VertId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Triangulation {
    /// Counter-clockwise seen from outside, like the faces they come from.
    pub tris: Vec<[VertId; 3]>,
    /// For each triangle, the face it came from.
    pub tri_face: Vec<FaceId>,
}

impl Triangulation {
    pub fn build(mesh: &PolyMesh) -> Self {
        let mut out = Triangulation::default();
        let mut scratch = Scratch::default();
        for (fid, face) in mesh.faces() {
            let pts: Vec<Vec3> = face.verts.iter().map(|v| mesh.vert(*v).unwrap_or_default()).collect();
            let start = out.tris.len();
            triangulate_polygon(&pts, &mut scratch, |a, b, c| {
                out.tris.push([face.verts[a], face.verts[b], face.verts[c]]);
            });
            out.tri_face.extend(std::iter::repeat_n(fid, out.tris.len() - start));
        }
        out
    }

    /// Triangle index ranges per face (faces appear in ascending id order).
    pub fn face_ranges(&self) -> Vec<(FaceId, std::ops::Range<usize>)> {
        let mut out: Vec<(FaceId, std::ops::Range<usize>)> = Vec::new();
        for (i, f) in self.tri_face.iter().enumerate() {
            match out.last_mut() {
                Some((last, r)) if last == f => r.end = i + 1,
                _ => out.push((*f, i..i + 1)),
            }
        }
        out
    }
}

#[derive(Default)]
struct Scratch {
    flat: Vec<[f64; 2]>,
    tris: Vec<u32>,
}

/// Triangulate one polygon given in order. Calls `emit(a, b, c)` with local indices.
fn triangulate_polygon(pts: &[Vec3], scratch: &mut Scratch, mut emit: impl FnMut(usize, usize, usize)) {
    let n = pts.len();
    match n {
        0..=2 => {}
        3 => emit(0, 1, 2),
        4 => {
            let normal = newell_normal(pts.iter().copied());
            // Option A: diagonal 0-2. Option B: diagonal 1-3.
            let score = |a: usize, b: usize, c: usize, d: usize| -> f32 {
                let t1 = (pts[b] - pts[a]).cross(pts[c] - pts[a]);
                let t2 = (pts[c] - pts[a]).cross(pts[d] - pts[a]);
                if t1.dot(normal) <= 0.0 || t2.dot(normal) <= 0.0 {
                    return f32::NEG_INFINITY; // one half would face backwards (concave quad)
                }
                // Prefer the shorter diagonal (better-shaped triangles).
                -(pts[c] - pts[a]).length_squared()
            };
            if score(1, 2, 3, 0) > score(0, 1, 2, 3) {
                emit(1, 2, 3);
                emit(1, 3, 0);
            } else {
                emit(0, 1, 2);
                emit(0, 2, 3);
            }
        }
        _ => {
            if is_convex(pts) {
                for i in 1..n - 1 {
                    emit(0, i, i + 1);
                }
                return;
            }
            let normal = newell_normal(pts.iter().copied()).as_dvec3().normalize_or_zero();
            let (u, v) = plane_basis(normal);
            scratch.flat.clear();
            scratch.flat.extend(pts.iter().map(|p| {
                let p = p.as_dvec3();
                [p.dot(u), p.dot(v)]
            }));
            scratch.tris.clear();
            let mut ec = earcut::Earcut::<f64>::new();
            ec.earcut(scratch.flat.iter().copied(), &[], &mut scratch.tris);
            if scratch.tris.len() / 3 != n - 2 {
                // Self-overlapping outline: fall back to a fan so nothing disappears.
                for i in 1..n - 1 {
                    emit(0, i, i + 1);
                }
                return;
            }
            for t in scratch.tris.chunks_exact(3) {
                let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                // Keep the face's winding.
                let tn = (pts[b] - pts[a]).cross(pts[c] - pts[a]).as_dvec3();
                if tn.dot(normal) >= 0.0 { emit(a, b, c) } else { emit(a, c, b) }
            }
        }
    }
}

fn is_convex(pts: &[Vec3]) -> bool {
    let n = pts.len();
    let normal = newell_normal(pts.iter().copied());
    (0..n).all(|i| {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let c = pts[(i + 2) % n];
        (b - a).cross(c - b).dot(normal) >= -1e-9
    })
}

/// Two unit vectors spanning the plane with normal `n`, so that (u, v, n) is right-handed.
fn plane_basis(n: DVec3) -> (DVec3, DVec3) {
    let helper = if n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let u = helper.cross(n).normalize_or(DVec3::X);
    let v = n.cross(u);
    (u, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concave_quad_uses_the_inside_diagonal() {
        // Dart shape: vertex 2 pokes inward.
        let pts =
            [Vec3::new(0.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0), Vec3::new(0.6, 0.0, -0.6), Vec3::new(0.0, 0.0, -2.0)];
        let normal = newell_normal(pts);
        let mut tris = vec![];
        triangulate_polygon(&pts, &mut Scratch::default(), |a, b, c| tris.push([a, b, c]));
        assert_eq!(tris.len(), 2);
        for t in tris {
            let tn = (pts[t[1]] - pts[t[0]]).cross(pts[t[2]] - pts[t[0]]);
            assert!(tn.dot(normal) > 0.0, "triangle {t:?} faces backwards");
        }
    }

    #[test]
    fn concave_ngon_area_is_preserved() {
        // An L shape (6 points), CCW seen from +Y.
        let pts = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 2.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(2.0, 0.0, 1.0),
            Vec3::new(2.0, 0.0, 0.0),
        ];
        let normal = newell_normal(pts);
        let mut area = 0.0;
        let mut count = 0;
        triangulate_polygon(&pts, &mut Scratch::default(), |a, b, c| {
            let tn = (pts[b] - pts[a]).cross(pts[c] - pts[a]);
            assert!(tn.dot(normal) > 0.0);
            area += tn.length() * 0.5;
            count += 1;
        });
        assert_eq!(count, 4);
        assert!((area - 3.0).abs() < 1e-5, "{area}");
    }
}
