//! Adjacency built on demand from the face lists (a half-edge view).

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::{EdgeKey, FaceId, PolyMesh, VertId};

#[derive(Clone, Debug, Default)]
pub struct Topology {
    /// Directed edge (from, to) -> the face that walks it in that direction.
    /// If two faces walk the same direction (a winding conflict), the first wins here.
    half_edges: FxHashMap<(VertId, VertId), FaceId>,
    edge_faces: BTreeMap<EdgeKey, SmallVec<[FaceId; 2]>>,
    vert_faces: BTreeMap<VertId, SmallVec<[FaceId; 6]>>,
    winding_conflicts: Vec<EdgeKey>,
}

impl Topology {
    pub fn build(mesh: &PolyMesh) -> Self {
        let mut t = Topology::default();
        for (fid, f) in mesh.faces() {
            let n = f.verts.len();
            for i in 0..n {
                let a = f.verts[i];
                let b = f.verts[(i + 1) % n];
                if t.half_edges.insert((a, b), fid).is_some() {
                    t.winding_conflicts.push(EdgeKey::new(a, b));
                    // Keep the first owner.
                    let first = t.edge_faces[&EdgeKey::new(a, b)][0];
                    t.half_edges.insert((a, b), first);
                }
                t.edge_faces.entry(EdgeKey::new(a, b)).or_default().push(fid);
                t.vert_faces.entry(a).or_default().push(fid);
            }
        }
        t.winding_conflicts.sort_unstable();
        t.winding_conflicts.dedup();
        t
    }

    pub fn faces_of_edge(&self, e: EdgeKey) -> &[FaceId] {
        self.edge_faces.get(&e).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn faces_of_vert(&self, v: VertId) -> &[FaceId] {
        self.vert_faces.get(&v).map(|f| f.as_slice()).unwrap_or(&[])
    }

    /// The face that walks `from -> to`, if any.
    pub fn face_of_half_edge(&self, from: VertId, to: VertId) -> Option<FaceId> {
        self.half_edges.get(&(from, to)).copied()
    }

    /// The face on the other side of `e` from `face` (only for manifold edges).
    pub fn opposite_face(&self, face: FaceId, e: EdgeKey) -> Option<FaceId> {
        match self.faces_of_edge(e) {
            [a, b] if *a == face => Some(*b),
            [a, b] if *b == face => Some(*a),
            _ => None,
        }
    }

    pub fn edges(&self) -> impl Iterator<Item = (EdgeKey, &[FaceId])> {
        self.edge_faces.iter().map(|(e, f)| (*e, f.as_slice()))
    }

    pub fn edge_count(&self) -> usize {
        self.edge_faces.len()
    }

    /// Edges used by exactly one face (holes / gaps).
    pub fn boundary_edges(&self) -> Vec<EdgeKey> {
        self.edge_faces.iter().filter(|(_, f)| f.len() == 1).map(|(e, _)| *e).collect()
    }

    /// Edges used by more than two faces.
    pub fn nonmanifold_edges(&self) -> Vec<EdgeKey> {
        self.edge_faces.iter().filter(|(_, f)| f.len() > 2).map(|(e, _)| *e).collect()
    }

    /// Edges where two faces disagree about which way is "out".
    pub fn winding_conflicts(&self) -> &[EdgeKey] {
        &self.winding_conflicts
    }

    /// Faces sharing an edge with `face`.
    pub fn neighbors(&self, mesh: &PolyMesh, face: FaceId) -> Vec<FaceId> {
        let Some(edges) = mesh.face_edges(face) else { return vec![] };
        let mut out: Vec<FaceId> =
            edges.iter().flat_map(|e| self.faces_of_edge(*e).iter().copied()).filter(|f| *f != face).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Groups of faces connected through shared edges (loose parts), each sorted.
    pub fn connected_components(&self, mesh: &PolyMesh) -> Vec<Vec<FaceId>> {
        let mut seen = std::collections::BTreeSet::new();
        let mut parts = Vec::new();
        for start in mesh.face_ids() {
            if !seen.insert(start) {
                continue;
            }
            let mut part = vec![start];
            let mut stack = vec![start];
            while let Some(f) = stack.pop() {
                for n in self.neighbors(mesh, f) {
                    if seen.insert(n) {
                        part.push(n);
                        stack.push(n);
                    }
                }
            }
            part.sort_unstable();
            parts.push(part);
        }
        parts
    }
}
