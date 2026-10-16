//! The document: every object, the palette and project settings.
//!
//! Fields are private to this crate: the only way to change a `Document` is a
//! [`Tx`](crate::Tx), which makes every change undoable by construction.
//! Parts are shared `Arc`s, so cloning a document (for previews, autosave or
//! history) is cheap.

use std::collections::BTreeMap;
use std::sync::Arc;

use ss_math::{Aabb, Transform};
use ss_mesh::{MaterialId, PolyMesh};

use crate::material::{Material, default_palette};
use crate::object::{Object, ObjectId, ObjectKind, ShapeSource};
use crate::scene::{DocMeta, ProjectTarget, SceneSettings};

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub(crate) meta: Arc<DocMeta>,
    pub(crate) scene: Arc<SceneSettings>,
    pub(crate) objects: BTreeMap<ObjectId, Arc<Object>>,
    pub(crate) roots: Arc<Vec<ObjectId>>,
    pub(crate) materials: BTreeMap<MaterialId, Arc<Material>>,
    /// Ids are never reused, even after undo.
    pub(crate) next_object: u32,
    pub(crate) next_material: u32,
}

impl Default for Document {
    fn default() -> Self {
        Self::new(ProjectTarget::Roblox)
    }
}

impl Document {
    /// A new, empty project with the default palette.
    pub fn new(target: ProjectTarget) -> Self {
        let mut doc = Self::empty(DocMeta::for_target(target));
        for m in default_palette() {
            let id = MaterialId(doc.next_material);
            doc.next_material += 1;
            doc.materials.insert(id, Arc::new(m));
        }
        doc
    }

    /// No objects and no materials (useful for loading and tests).
    pub fn empty(meta: DocMeta) -> Self {
        Self {
            meta: Arc::new(meta),
            scene: Arc::new(SceneSettings::default()),
            objects: BTreeMap::new(),
            roots: Arc::new(Vec::new()),
            materials: BTreeMap::new(),
            next_object: 0,
            next_material: 0,
        }
    }

    /// Rebuild a document from loaded parts. Ids are kept; parent/child links are
    /// taken from the objects. Used by the file loader (which validates first).
    pub fn from_parts(
        meta: DocMeta,
        scene: SceneSettings,
        objects: BTreeMap<ObjectId, Object>,
        roots: Vec<ObjectId>,
        materials: BTreeMap<MaterialId, Material>,
        next_object: u32,
        next_material: u32,
    ) -> Self {
        let next_object = next_object.max(objects.keys().last().map_or(0, |id| id.0 + 1));
        let next_material = next_material.max(materials.keys().last().map_or(0, |id| id.0 + 1));
        Self {
            meta: Arc::new(meta),
            scene: Arc::new(scene),
            objects: objects.into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            roots: Arc::new(roots),
            materials: materials.into_iter().map(|(k, v)| (k, Arc::new(v))).collect(),
            next_object,
            next_material,
        }
    }

    pub fn meta(&self) -> &DocMeta {
        &self.meta
    }

    pub fn scene(&self) -> &SceneSettings {
        &self.scene
    }

    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id).map(|o| o.as_ref())
    }

    pub fn object_arc(&self, id: ObjectId) -> Option<&Arc<Object>> {
        self.objects.get(&id)
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects.contains_key(&id)
    }

    /// All objects in ascending id order.
    pub fn objects(&self) -> impl DoubleEndedIterator<Item = (ObjectId, &Object)> + ExactSizeIterator {
        self.objects.iter().map(|(k, v)| (*k, v.as_ref()))
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn roots(&self) -> &[ObjectId] {
        &self.roots
    }

    pub fn children(&self, id: ObjectId) -> &[ObjectId] {
        self.object(id).map(|o| o.children.as_slice()).unwrap_or(&[])
    }

    /// Siblings list that contains `id` (its parent's children, or the roots).
    pub fn siblings(&self, id: ObjectId) -> &[ObjectId] {
        match self.object(id).and_then(|o| o.parent) {
            Some(p) => self.children(p),
            None => self.roots(),
        }
    }

    pub fn materials(&self) -> impl DoubleEndedIterator<Item = (MaterialId, &Material)> + ExactSizeIterator {
        self.materials.iter().map(|(k, v)| (*k, v.as_ref()))
    }

    pub fn material(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(&id).map(|m| m.as_ref())
    }

    /// The material new shapes get.
    pub fn default_material(&self) -> Option<MaterialId> {
        self.materials.keys().next().copied()
    }

    pub fn next_object_id(&self) -> u32 {
        self.next_object
    }

    pub fn next_material_id(&self) -> u32 {
        self.next_material
    }

    /// Objects in outliner order (depth-first, parents before children).
    pub fn walk(&self) -> Vec<ObjectId> {
        let mut out = Vec::with_capacity(self.objects.len());
        let mut stack: Vec<ObjectId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            out.push(id);
            stack.extend(self.children(id).iter().rev().copied());
        }
        out
    }

    /// `id` and everything below it, parents first.
    pub fn subtree(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(i) = stack.pop() {
            if self.contains(i) {
                out.push(i);
                stack.extend(self.children(i).iter().rev().copied());
            }
        }
        out
    }

    /// Parent, grandparent, ... (nearest first).
    pub fn ancestors(&self, id: ObjectId) -> Vec<ObjectId> {
        let mut out = Vec::new();
        let mut cur = self.object(id).and_then(|o| o.parent);
        while let Some(p) = cur {
            if out.contains(&p) {
                break; // corrupt cycle; never loop forever
            }
            out.push(p);
            cur = self.object(p).and_then(|o| o.parent);
        }
        out
    }

    pub fn is_ancestor(&self, maybe_ancestor: ObjectId, of: ObjectId) -> bool {
        self.ancestors(of).contains(&maybe_ancestor)
    }

    /// Object-to-world transform.
    pub fn world_transform(&self, id: ObjectId) -> Transform {
        let mut t = self.object(id).map(|o| o.transform).unwrap_or_default();
        for a in self.ancestors(id) {
            if let Some(o) = self.object(a) {
                t = o.transform.mul(&t);
            }
        }
        t
    }

    /// Transform of the parent space (identity for root objects).
    pub fn parent_world_transform(&self, id: ObjectId) -> Transform {
        match self.object(id).and_then(|o| o.parent) {
            Some(p) => self.world_transform(p),
            None => Transform::IDENTITY,
        }
    }

    /// Visible if it and all its ancestors are visible.
    pub fn is_visible(&self, id: ObjectId) -> bool {
        self.object(id).is_some_and(|o| o.visible)
            && self.ancestors(id).iter().all(|a| self.object(*a).is_some_and(|o| o.visible))
    }

    pub fn is_locked(&self, id: ObjectId) -> bool {
        self.object(id).is_some_and(|o| o.locked)
            || self.ancestors(id).iter().any(|a| self.object(*a).is_some_and(|o| o.locked))
    }

    /// The shape's own mesh in object space (pivot applied, before effects). Builds
    /// primitives on the fly; use an [`Evaluator`](crate::Evaluator) when calling this every frame.
    pub fn mesh_of(&self, id: ObjectId) -> Option<Arc<PolyMesh>> {
        Some(self.object(id)?.shape()?.base_mesh())
    }

    /// World-space bounds of an object and its children (shapes only).
    pub fn world_bounds(&self, id: ObjectId) -> Aabb {
        let mut b = Aabb::EMPTY;
        for i in self.subtree(id) {
            if let Some(m) = self.mesh_of(i) {
                b = b.union(&m.bounds().transformed(&self.world_transform(i).matrix()));
            }
        }
        b
    }

    /// "Block 1", "Block 2", ... the first free name with this base.
    pub fn unique_name(&self, base: &str) -> String {
        let taken: std::collections::BTreeSet<&str> = self.objects.values().map(|o| o.name.as_str()).collect();
        (1..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(n.as_str())).unwrap()
    }

    /// Number of objects using a material (as their own or on any face).
    pub fn material_users(&self, mat: MaterialId) -> usize {
        self.objects
            .values()
            .filter(|o| match &o.kind {
                ObjectKind::Shape(s) => {
                    s.material == mat
                        || matches!(&s.source, ShapeSource::Mesh(m) if m.faces().any(|(_, f)| f.material == Some(mat)))
                }
                _ => false,
            })
            .count()
    }

    /// Structural problems (empty when healthy). Checked by tests and the file loader.
    pub fn check_invariants(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for r in self.roots.iter() {
            match self.object(*r) {
                None => problems.push(format!("root {r} does not exist")),
                Some(o) if o.parent.is_some() => problems.push(format!("root {r} has a parent")),
                _ => {}
            }
        }
        for (id, o) in self.objects() {
            match o.parent {
                None if !self.roots.contains(&id) => problems.push(format!("{id} has no parent and is not a root")),
                Some(p) => match self.object(p) {
                    None => problems.push(format!("{id} has missing parent {p}")),
                    Some(po) if !po.children.contains(&id) => problems.push(format!("{id} not in parent's children")),
                    _ => {}
                },
                _ => {}
            }
            for c in &o.children {
                if self.object(*c).and_then(|co| co.parent) != Some(id) {
                    problems.push(format!("child {c} of {id} doesn't point back"));
                }
            }
            if let Some(s) = o.shape()
                && !self.materials.contains_key(&s.material)
            {
                problems.push(format!("{id} uses missing material {:?}", s.material));
            }
            if id.0 >= self.next_object {
                problems.push(format!("{id} >= next object id"));
            }
        }
        for id in self.walk() {
            if !seen.insert(id) {
                problems.push(format!("{id} reachable twice"));
            }
        }
        if seen.len() != self.objects.len() {
            problems.push(format!("{} objects but {} reachable", self.objects.len(), seen.len()));
        }
        problems
    }
}
