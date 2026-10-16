//! Bounding volume hierarchy over the shared triangulation, for picking and
//! "stop before the far side" ray casts.

use glam::Vec3;
use ss_math::{Aabb, Ray};

use crate::triangulate::Triangulation;
use crate::{FaceId, PolyMesh};

const LEAF_SIZE: usize = 4;

#[derive(Clone, Debug)]
enum Node {
    Leaf { bounds: Aabb, start: u32, count: u32 },
    Inner { bounds: Aabb, left: u32, right: u32 },
}

impl Node {
    fn bounds(&self) -> &Aabb {
        match self {
            Node::Leaf { bounds, .. } | Node::Inner { bounds, .. } => bounds,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    /// Triangle corner positions, reordered for locality.
    tris: Vec<[Vec3; 3]>,
    /// For each reordered triangle: (index in the triangulation, face).
    tri_ids: Vec<(u32, FaceId)>,
}

/// The closest hit of a ray against a mesh (in the mesh's own space).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshHit {
    pub t: f32,
    pub point: Vec3,
    /// Geometric normal of the hit triangle (pointing out for well-wound meshes).
    pub normal: Vec3,
    pub face: FaceId,
    /// Index into the mesh's triangulation.
    pub tri: u32,
}

impl Bvh {
    pub fn build(mesh: &PolyMesh, tri: &Triangulation) -> Self {
        let mut items: Vec<(Aabb, Vec3, [Vec3; 3], u32, FaceId)> = tri
            .tris
            .iter()
            .zip(&tri.tri_face)
            .enumerate()
            .map(|(i, (t, f))| {
                let p = t.map(|v| mesh.vert(v).unwrap_or_default());
                let b = Aabb::from_points(p);
                (b, b.center(), p, i as u32, *f)
            })
            .collect();
        let mut bvh = Bvh::default();
        if items.is_empty() {
            return bvh;
        }
        bvh.nodes.push(Node::Leaf { bounds: Aabb::EMPTY, start: 0, count: 0 });
        bvh.build_node(0, &mut items[..], 0);
        for it in &items {
            bvh.tris.push(it.2);
            bvh.tri_ids.push((it.3, it.4));
        }
        bvh
    }

    fn build_node(&mut self, node: usize, items: &mut [(Aabb, Vec3, [Vec3; 3], u32, FaceId)], offset: usize) {
        let bounds = items.iter().fold(Aabb::EMPTY, |acc, it| acc.union(&it.0));
        if items.len() <= LEAF_SIZE {
            self.nodes[node] = Node::Leaf { bounds, start: offset as u32, count: items.len() as u32 };
            return;
        }
        let centers = Aabb::from_points(items.iter().map(|it| it.1));
        let axis = centers.size().max_position();
        items.sort_unstable_by(|a, b| a.1[axis].total_cmp(&b.1[axis]));
        let mid = items.len() / 2;
        let left = self.nodes.len();
        self.nodes.push(Node::Leaf { bounds: Aabb::EMPTY, start: 0, count: 0 });
        let right = self.nodes.len();
        self.nodes.push(Node::Leaf { bounds: Aabb::EMPTY, start: 0, count: 0 });
        self.nodes[node] = Node::Inner { bounds, left: left as u32, right: right as u32 };
        let (l, r) = items.split_at_mut(mid);
        self.build_node(left, l, offset);
        self.build_node(right, r, offset + mid);
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map(|n| *n.bounds()).unwrap_or(Aabb::EMPTY)
    }

    /// Closest hit along the ray (two-sided).
    pub fn raycast(&self, ray: &Ray) -> Option<MeshHit> {
        self.raycast_filtered(ray, |_| true)
    }

    /// Closest hit among triangles whose face passes `filter`.
    pub fn raycast_filtered(&self, ray: &Ray, filter: impl Fn(FaceId) -> bool) -> Option<MeshHit> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best: Option<MeshHit> = None;
        let mut stack = vec![0u32];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            let Some(enter) = ray.intersect_aabb(node.bounds()) else { continue };
            if best.is_some_and(|b| enter > b.t) {
                continue;
            }
            match node {
                Node::Leaf { start, count, .. } => {
                    for i in *start..*start + *count {
                        let (tri_index, face) = self.tri_ids[i as usize];
                        if !filter(face) {
                            continue;
                        }
                        let [a, b, c] = self.tris[i as usize];
                        if let Some(h) = ray.intersect_triangle(a, b, c)
                            && best.is_none_or(|bh| h.t < bh.t)
                        {
                            best = Some(MeshHit {
                                t: h.t,
                                point: ray.at(h.t),
                                normal: (b - a).cross(c - a).normalize_or_zero(),
                                face,
                                tri: tri_index,
                            });
                        }
                    }
                }
                Node::Inner { left, right, .. } => {
                    stack.push(*left);
                    stack.push(*right);
                }
            }
        }
        best
    }

    /// Every hit along the ray, nearest first (used for "how far until the far side?").
    pub fn raycast_all(&self, ray: &Ray) -> Vec<MeshHit> {
        let mut hits: Vec<MeshHit> = self
            .tris
            .iter()
            .zip(&self.tri_ids)
            .filter_map(|([a, b, c], (tri, face))| {
                ray.intersect_triangle(*a, *b, *c).map(|h| MeshHit {
                    t: h.t,
                    point: ray.at(h.t),
                    normal: (*b - *a).cross(*c - *a).normalize_or_zero(),
                    face: *face,
                    tri: *tri,
                })
            })
            .collect();
        hits.sort_by(|a, b| a.t.total_cmp(&b.t));
        hits
    }
}
