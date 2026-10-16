//! The editable polygon mesh.
//!
//! Canonical data is just points plus faces (lists of point ids, counter-clockwise
//! seen from outside). Everything derived — half-edge topology, the single shared
//! triangulation, the BVH — is cached lazily and dropped on any change.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use glam::{Mat4, Vec3};
use smallvec::SmallVec;
use ss_math::{Aabb, newell_normal};

use crate::bvh::Bvh;
use crate::health::MeshHealth;
use crate::store::ElementStore;
use crate::topology::Topology;
use crate::triangulate::Triangulation;
use crate::{EdgeKey, FaceId, MaterialId, MeshError, VertId};

/// Global revision counter. Every mesh change takes a fresh, globally unique
/// revision, so `(object, rev)` is a safe cache key for GPU buffers and evaluation.
static NEXT_REV: AtomicU64 = AtomicU64::new(1);

pub fn next_rev() -> u64 {
    NEXT_REV.fetch_add(1, Ordering::Relaxed)
}

pub type FaceVerts = SmallVec<[VertId; 4]>;

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Counter-clockwise when seen from outside the shape.
    pub verts: FaceVerts,
    /// `None` uses the object's material.
    pub material: Option<MaterialId>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeAttrs {
    /// Stays sharp in "Smooth with Sharp Edges" and Smooth & Round.
    pub sharp: bool,
}

#[derive(Default)]
struct MeshCache {
    triangulation: OnceLock<Arc<Triangulation>>,
    topology: OnceLock<Arc<Topology>>,
    bvh: OnceLock<Arc<Bvh>>,
    health: OnceLock<Arc<MeshHealth>>,
}

impl Clone for MeshCache {
    fn clone(&self) -> Self {
        let c = MeshCache::default();
        if let Some(v) = self.triangulation.get() {
            let _ = c.triangulation.set(v.clone());
        }
        if let Some(v) = self.topology.get() {
            let _ = c.topology.set(v.clone());
        }
        if let Some(v) = self.bvh.get() {
            let _ = c.bvh.set(v.clone());
        }
        if let Some(v) = self.health.get() {
            let _ = c.health.set(v.clone());
        }
        c
    }
}

impl std::fmt::Debug for MeshCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MeshCache")
    }
}

#[derive(Clone, Debug)]
pub struct PolyMesh {
    verts: ElementStore<VertId, Vec3>,
    faces: ElementStore<FaceId, Face>,
    /// Only non-default attributes are stored.
    edge_attrs: BTreeMap<EdgeKey, EdgeAttrs>,
    rev: u64,
    cache: MeshCache,
}

impl Default for PolyMesh {
    fn default() -> Self {
        Self::new()
    }
}

/// Geometry equality (ignores revision and caches).
impl PartialEq for PolyMesh {
    fn eq(&self, other: &Self) -> bool {
        self.verts == other.verts && self.faces == other.faces && self.edge_attrs == other.edge_attrs
    }
}

impl PolyMesh {
    pub fn new() -> Self {
        Self {
            verts: ElementStore::new(),
            faces: ElementStore::new(),
            edge_attrs: BTreeMap::new(),
            rev: next_rev(),
            cache: MeshCache::default(),
        }
    }

    /// Build from plain arrays (faces index into `points`). Ids are assigned in order.
    pub fn from_arrays(points: &[Vec3], faces: &[&[u32]]) -> Result<Self, MeshError> {
        let mut m = Self::new();
        let ids: Vec<VertId> = points.iter().map(|p| m.add_vert(*p)).collect();
        for f in faces {
            let vs: Vec<VertId> = f
                .iter()
                .map(|&i| ids.get(i as usize).copied().ok_or(MeshError::MissingVert(VertId(i))))
                .collect::<Result<_, _>>()?;
            m.add_face(&vs, None)?;
        }
        Ok(m)
    }

    fn touch(&mut self) {
        self.rev = next_rev();
        self.cache = MeshCache::default();
    }

    /// Globally unique revision; changes on every edit.
    pub fn rev(&self) -> u64 {
        self.rev
    }

    // ---- points -------------------------------------------------------------

    pub fn add_vert(&mut self, p: Vec3) -> VertId {
        self.touch();
        self.verts.insert(p)
    }

    pub fn insert_vert_with_id(&mut self, id: VertId, p: Vec3) {
        self.touch();
        self.verts.insert_with_id(id, p);
    }

    pub fn vert(&self, id: VertId) -> Option<Vec3> {
        self.verts.get(id).copied()
    }

    pub fn set_vert(&mut self, id: VertId, p: Vec3) -> Result<(), MeshError> {
        let slot = self.verts.get_mut(id).ok_or(MeshError::MissingVert(id))?;
        *slot = p;
        self.touch();
        Ok(())
    }

    pub fn verts(&self) -> impl DoubleEndedIterator<Item = (VertId, Vec3)> + ExactSizeIterator + '_ {
        self.verts.iter().map(|(k, v)| (k, *v))
    }

    pub fn vert_ids(&self) -> impl DoubleEndedIterator<Item = VertId> + ExactSizeIterator + '_ {
        self.verts.keys()
    }

    pub fn vert_count(&self) -> usize {
        self.verts.len()
    }

    pub fn contains_vert(&self, id: VertId) -> bool {
        self.verts.contains(id)
    }

    /// Removes a point that no face uses.
    pub fn remove_vert(&mut self, id: VertId) -> Result<Vec3, MeshError> {
        if self.faces.values().any(|f| f.verts.contains(&id)) {
            return Err(MeshError::VertInUse(id));
        }
        let p = self.verts.remove(id).ok_or(MeshError::MissingVert(id))?;
        self.edge_attrs.retain(|e, _| !e.contains(id));
        self.touch();
        Ok(p)
    }

    /// Removes points no face uses. Returns how many were removed.
    pub fn remove_loose_verts(&mut self) -> usize {
        let used: std::collections::BTreeSet<VertId> =
            self.faces.values().flat_map(|f| f.verts.iter().copied()).collect();
        let before = self.verts.len();
        self.verts.retain(|id, _| used.contains(&id));
        let removed = before - self.verts.len();
        if removed > 0 {
            self.edge_attrs.retain(|e, _| used.contains(&e.a) && used.contains(&e.b));
            self.touch();
        }
        removed
    }

    // ---- faces --------------------------------------------------------------

    /// Adds a face. Needs at least 3 distinct, existing points.
    pub fn add_face(&mut self, verts: &[VertId], material: Option<MaterialId>) -> Result<FaceId, MeshError> {
        self.check_face(verts)?;
        self.touch();
        Ok(self.faces.insert(Face { verts: verts.iter().copied().collect(), material }))
    }

    pub fn insert_face_with_id(&mut self, id: FaceId, face: Face) -> Result<(), MeshError> {
        self.check_face(&face.verts)?;
        self.touch();
        self.faces.insert_with_id(id, face);
        Ok(())
    }

    fn check_face(&self, verts: &[VertId]) -> Result<(), MeshError> {
        if verts.len() < 3 {
            return Err(MeshError::TooFewVerts(verts.len()));
        }
        for (i, v) in verts.iter().enumerate() {
            if !self.verts.contains(*v) {
                return Err(MeshError::MissingVert(*v));
            }
            if verts[i + 1..].contains(v) {
                return Err(MeshError::RepeatedVert(*v));
            }
        }
        Ok(())
    }

    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id)
    }

    pub fn faces(&self) -> impl DoubleEndedIterator<Item = (FaceId, &Face)> + ExactSizeIterator {
        self.faces.iter()
    }

    pub fn face_ids(&self) -> impl DoubleEndedIterator<Item = FaceId> + ExactSizeIterator + '_ {
        self.faces.keys()
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    pub fn contains_face(&self, id: FaceId) -> bool {
        self.faces.contains(id)
    }

    pub fn remove_face(&mut self, id: FaceId) -> Result<Face, MeshError> {
        let f = self.faces.remove(id).ok_or(MeshError::MissingFace(id))?;
        self.touch();
        Ok(f)
    }

    /// Replace a face's point list (keeps its id and material).
    pub fn set_face_verts(&mut self, id: FaceId, verts: &[VertId]) -> Result<(), MeshError> {
        self.check_face(verts)?;
        let f = self.faces.get_mut(id).ok_or(MeshError::MissingFace(id))?;
        f.verts = verts.iter().copied().collect();
        self.touch();
        Ok(())
    }

    pub fn set_face_material(&mut self, id: FaceId, material: Option<MaterialId>) -> Result<(), MeshError> {
        let f = self.faces.get_mut(id).ok_or(MeshError::MissingFace(id))?;
        if f.material != material {
            f.material = material;
            self.touch();
        }
        Ok(())
    }

    /// Reverse a face's winding (turns it inside-out).
    pub fn flip_face(&mut self, id: FaceId) -> Result<(), MeshError> {
        let f = self.faces.get_mut(id).ok_or(MeshError::MissingFace(id))?;
        f.verts.reverse();
        self.touch();
        Ok(())
    }

    pub fn face_positions(&self, id: FaceId) -> Option<SmallVec<[Vec3; 4]>> {
        let f = self.faces.get(id)?;
        f.verts.iter().map(|v| self.verts.get(*v).copied()).collect()
    }

    /// Unit normal (Newell). Zero for degenerate faces.
    pub fn face_normal(&self, id: FaceId) -> Option<Vec3> {
        self.face_positions(id).map(|p| newell_normal(p.iter().copied()).normalize_or_zero())
    }

    /// Area-weighted (unnormalized) Newell vector; its length is twice the area for planar faces.
    pub fn face_area(&self, id: FaceId) -> Option<f32> {
        self.face_positions(id).map(|p| newell_normal(p.iter().copied()).length() * 0.5)
    }

    pub fn face_center(&self, id: FaceId) -> Option<Vec3> {
        let p = self.face_positions(id)?;
        Some(p.iter().copied().sum::<Vec3>() / p.len() as f32)
    }

    /// The face's edges in winding order.
    pub fn face_edges(&self, id: FaceId) -> Option<SmallVec<[EdgeKey; 4]>> {
        let f = self.faces.get(id)?;
        let n = f.verts.len();
        Some((0..n).map(|i| EdgeKey::new(f.verts[i], f.verts[(i + 1) % n])).collect())
    }

    /// All unique edges, sorted.
    pub fn edges(&self) -> Vec<EdgeKey> {
        let mut set: Vec<EdgeKey> = self
            .faces
            .values()
            .flat_map(|f| {
                let n = f.verts.len();
                (0..n).map(move |i| EdgeKey::new(f.verts[i], f.verts[(i + 1) % n]))
            })
            .collect();
        set.sort_unstable();
        set.dedup();
        set
    }

    pub fn edge_attrs(&self, e: EdgeKey) -> EdgeAttrs {
        self.edge_attrs.get(&e).copied().unwrap_or_default()
    }

    pub fn set_edge_sharp(&mut self, e: EdgeKey, sharp: bool) {
        let attrs = EdgeAttrs { sharp };
        if attrs == EdgeAttrs::default() {
            if self.edge_attrs.remove(&e).is_some() {
                self.touch();
            }
        } else if self.edge_attrs.insert(e, attrs) != Some(attrs) {
            self.touch();
        }
    }

    pub fn sharp_edges(&self) -> impl Iterator<Item = EdgeKey> + '_ {
        self.edge_attrs.iter().filter(|(_, a)| a.sharp).map(|(e, _)| *e)
    }

    pub fn next_vert_id(&self) -> u32 {
        self.verts.next_id()
    }

    pub fn next_face_id(&self) -> u32 {
        self.faces.next_id()
    }

    /// Make sure future fresh ids are at least these (restoring saved counters).
    pub fn bump_next_ids(&mut self, next_vert: u32, next_face: u32) {
        self.verts.bump_next_id(next_vert);
        self.faces.bump_next_id(next_face);
    }

    // ---- whole-mesh -----------------------------------------------------------

    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(self.verts.values().copied())
    }

    /// Apply an affine transform to every point. A mirroring transform
    /// (negative determinant) also reverses every face so nothing turns inside out.
    pub fn transform(&mut self, m: &Mat4) {
        for (_, p) in self.verts.iter_mut() {
            *p = m.transform_point3(*p);
        }
        if m.determinant() < 0.0 {
            for (_, f) in self.faces.iter_mut() {
                f.verts.reverse();
            }
        }
        self.touch();
    }

    /// Append another mesh (ids are remapped). Returns the new ids of its faces, in order.
    pub fn append(&mut self, other: &PolyMesh) -> Vec<FaceId> {
        let mut map = BTreeMap::new();
        for (id, p) in other.verts() {
            map.insert(id, self.verts.insert(p));
        }
        let mut out = Vec::with_capacity(other.face_count());
        for (_, f) in other.faces() {
            let verts: FaceVerts = f.verts.iter().map(|v| map[v]).collect();
            out.push(self.faces.insert(Face { verts, material: f.material }));
        }
        for (e, a) in &other.edge_attrs {
            self.edge_attrs.insert(EdgeKey::new(map[&e.a], map[&e.b]), *a);
        }
        self.touch();
        out
    }

    /// Signed volume (positive for a closed, outward-wound shape).
    pub fn signed_volume(&self) -> f64 {
        let tri = self.triangulation();
        tri.tris
            .iter()
            .map(|t| {
                let a = self.verts.get(t[0]).copied().unwrap_or_default().as_dvec3();
                let b = self.verts.get(t[1]).copied().unwrap_or_default().as_dvec3();
                let c = self.verts.get(t[2]).copied().unwrap_or_default().as_dvec3();
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangulation().tris.len()
    }

    // ---- cached derived data -------------------------------------------------------

    /// The one triangulation used by drawing, picking, export and booleans.
    pub fn triangulation(&self) -> Arc<Triangulation> {
        self.cache.triangulation.get_or_init(|| Arc::new(Triangulation::build(self))).clone()
    }

    pub fn topology(&self) -> Arc<Topology> {
        self.cache.topology.get_or_init(|| Arc::new(Topology::build(self))).clone()
    }

    pub fn bvh(&self) -> Arc<Bvh> {
        self.cache.bvh.get_or_init(|| Arc::new(Bvh::build(self, &self.triangulation()))).clone()
    }

    pub fn health(&self) -> Arc<MeshHealth> {
        self.cache.health.get_or_init(|| Arc::new(MeshHealth::compute(self))).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> PolyMesh {
        PolyMesh::from_arrays(
            &[Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0)],
            &[&[0, 1, 2, 3]],
        )
        .unwrap()
    }

    #[test]
    fn rev_changes_and_caches_reset() {
        let mut m = quad();
        let r0 = m.rev();
        let t0 = m.triangulation();
        assert_eq!(t0.tris.len(), 2);
        m.set_vert(VertId(0), Vec3::new(0.0, 0.5, 0.0)).unwrap();
        assert_ne!(m.rev(), r0);
        assert!(!Arc::ptr_eq(&t0, &m.triangulation()));
    }

    #[test]
    fn face_validation() {
        let mut m = quad();
        assert!(matches!(m.add_face(&[VertId(0), VertId(1)], None), Err(MeshError::TooFewVerts(2))));
        assert!(matches!(m.add_face(&[VertId(0), VertId(1), VertId(0)], None), Err(MeshError::RepeatedVert(_))));
        assert!(matches!(m.add_face(&[VertId(0), VertId(1), VertId(9)], None), Err(MeshError::MissingVert(_))));
        assert!(matches!(m.remove_vert(VertId(0)), Err(MeshError::VertInUse(_))));
    }

    #[test]
    fn normal_center_area() {
        let m = quad();
        assert!((m.face_normal(FaceId(0)).unwrap() - Vec3::Y).length() < 1e-6);
        assert!((m.face_center(FaceId(0)).unwrap() - Vec3::new(0.5, 0.0, 0.5)).length() < 1e-6);
        assert!((m.face_area(FaceId(0)).unwrap() - 1.0).abs() < 1e-6);
        assert_eq!(m.edges().len(), 4);
    }

    #[test]
    fn mirror_transform_keeps_outward_winding() {
        let mut m = quad();
        m.transform(&Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!((m.face_normal(FaceId(0)).unwrap() - Vec3::Y).length() < 1e-6);
    }

    #[test]
    fn geometry_equality_ignores_rev() {
        let a = quad();
        let b = quad();
        assert_ne!(a.rev(), b.rev());
        assert_eq!(a, b);
    }
}
