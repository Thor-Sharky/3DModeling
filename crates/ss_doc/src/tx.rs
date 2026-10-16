//! Transactions: the only way to change a document.
//!
//! A `Tx` records the "before" `Arc` of every part it touches (on first touch),
//! then mutates copy-on-write. `commit` returns a [`DocDelta`] (before + after
//! pointers) that undo/redo simply swap back in — exact by construction. A Tx
//! that is dropped without `commit` rolls back.

use std::collections::BTreeMap;
use std::sync::Arc;

use ss_math::Transform;
use ss_mesh::{MaterialId, PolyMesh};

use crate::material::Material;
use crate::object::{Object, ObjectId, ObjectKind, ShapeSource};
use crate::scene::{DocMeta, SceneSettings};
use crate::{DocError, Document};

/// The touched parts of a document at one moment. `None` inside the maps means
/// "did not exist".
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub(crate) meta: Option<Arc<DocMeta>>,
    pub(crate) scene: Option<Arc<SceneSettings>>,
    pub(crate) roots: Option<Arc<Vec<ObjectId>>>,
    pub(crate) objects: BTreeMap<ObjectId, Option<Arc<Object>>>,
    pub(crate) materials: BTreeMap<MaterialId, Option<Arc<Material>>>,
}

impl Snapshot {
    fn is_empty(&self) -> bool {
        self.meta.is_none()
            && self.scene.is_none()
            && self.roots.is_none()
            && self.objects.is_empty()
            && self.materials.is_empty()
    }

    fn restore_into(&self, doc: &mut Document) {
        if let Some(m) = &self.meta {
            doc.meta = m.clone();
        }
        if let Some(s) = &self.scene {
            doc.scene = s.clone();
        }
        if let Some(r) = &self.roots {
            doc.roots = r.clone();
        }
        for (id, o) in &self.objects {
            match o {
                Some(o) => {
                    doc.objects.insert(*id, o.clone());
                }
                None => {
                    doc.objects.remove(id);
                }
            }
        }
        for (id, m) in &self.materials {
            match m {
                Some(m) => {
                    doc.materials.insert(*id, m.clone());
                }
                None => {
                    doc.materials.remove(id);
                }
            }
        }
    }
}

/// What one committed transaction changed.
#[derive(Clone, Debug, Default)]
pub struct DocDelta {
    pub(crate) before: Snapshot,
    pub(crate) after: Snapshot,
}

impl DocDelta {
    /// Put the document back the way it was before (undo).
    pub fn apply_before(&self, doc: &mut Document) {
        self.before.restore_into(doc);
    }

    /// Re-apply the change (redo).
    pub fn apply_after(&self, doc: &mut Document) {
        self.after.restore_into(doc);
    }

    /// Objects created, changed or removed.
    pub fn touched_objects(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.before.objects.keys().copied()
    }

    /// Combine with a later delta into one step (used to merge slider drags).
    pub fn merge(mut self, later: DocDelta) -> DocDelta {
        let DocDelta { before: lb, after: la } = later;
        self.before.meta = self.before.meta.or(lb.meta);
        self.before.scene = self.before.scene.or(lb.scene);
        self.before.roots = self.before.roots.or(lb.roots);
        for (k, v) in lb.objects {
            self.before.objects.entry(k).or_insert(v);
        }
        for (k, v) in lb.materials {
            self.before.materials.entry(k).or_insert(v);
        }
        if la.meta.is_some() {
            self.after.meta = la.meta;
        }
        if la.scene.is_some() {
            self.after.scene = la.scene;
        }
        if la.roots.is_some() {
            self.after.roots = la.roots;
        }
        self.after.objects.extend(la.objects);
        self.after.materials.extend(la.materials);
        self
    }

    /// Rough memory held only by this step (for the history budget).
    pub fn approx_bytes(&self) -> usize {
        let obj_bytes = |o: &Option<Arc<Object>>| -> usize {
            match o.as_deref().map(|o| &o.kind) {
                Some(ObjectKind::Shape(s)) => match &s.source {
                    ShapeSource::Mesh(m) => 256 + m.vert_count() * 16 + m.face_count() * 48,
                    ShapeSource::Primitive(_) => 256,
                },
                _ => 128,
            }
        };
        self.before.objects.values().map(obj_bytes).sum::<usize>()
            + self.after.objects.values().map(obj_bytes).sum::<usize>()
            + 64 * (self.before.materials.len() + self.after.materials.len())
            + 128
    }
}

pub struct Tx<'d> {
    doc: &'d mut Document,
    label: String,
    before: Snapshot,
    done: bool,
}

impl Document {
    /// Start a change. Commit it to get an undoable delta; drop it to roll back.
    pub fn begin(&mut self, label: impl Into<String>) -> Tx<'_> {
        Tx { doc: self, label: label.into(), before: Snapshot::default(), done: false }
    }
}

impl<'d> Tx<'d> {
    /// The document as it is right now (including this transaction's edits).
    pub fn doc(&self) -> &Document {
        self.doc
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    fn touch_object(&mut self, id: ObjectId) {
        if !self.before.objects.contains_key(&id) {
            self.before.objects.insert(id, self.doc.objects.get(&id).cloned());
        }
    }

    fn touch_roots(&mut self) {
        if self.before.roots.is_none() {
            self.before.roots = Some(self.doc.roots.clone());
        }
    }

    fn touch_material(&mut self, id: MaterialId) {
        if !self.before.materials.contains_key(&id) {
            self.before.materials.insert(id, self.doc.materials.get(&id).cloned());
        }
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Result<&mut Object, DocError> {
        if !self.doc.objects.contains_key(&id) {
            return Err(DocError::MissingObject(id));
        }
        self.touch_object(id);
        Ok(Arc::make_mut(self.doc.objects.get_mut(&id).expect("checked above")))
    }

    /// Add an object under `parent` (or at the top level) at `index` (or at the end).
    pub fn add_object(
        &mut self,
        mut obj: Object,
        parent: Option<ObjectId>,
        index: Option<usize>,
    ) -> Result<ObjectId, DocError> {
        if let Some(p) = parent
            && !self.doc.contains(p)
        {
            return Err(DocError::MissingObject(p));
        }
        if let ObjectKind::Shape(s) = &obj.kind
            && !self.doc.materials.contains_key(&s.material)
        {
            return Err(DocError::MissingMaterial(s.material));
        }
        let id = ObjectId(self.doc.next_object);
        self.doc.next_object += 1;
        let children = std::mem::take(&mut obj.children);
        debug_assert!(children.is_empty(), "add children with their own add_object calls");
        obj.parent = parent;
        self.touch_object(id);
        self.doc.objects.insert(id, Arc::new(obj));
        self.link(id, parent, index)?;
        Ok(id)
    }

    fn link(&mut self, id: ObjectId, parent: Option<ObjectId>, index: Option<usize>) -> Result<(), DocError> {
        match parent {
            Some(p) => {
                let po = self.object_mut(p)?;
                let i = index.unwrap_or(po.children.len()).min(po.children.len());
                po.children.insert(i, id);
            }
            None => {
                self.touch_roots();
                let roots = Arc::make_mut(&mut self.doc.roots);
                let i = index.unwrap_or(roots.len()).min(roots.len());
                roots.insert(i, id);
            }
        }
        Ok(())
    }

    fn unlink(&mut self, id: ObjectId) -> Result<(), DocError> {
        let parent = self.doc.object(id).ok_or(DocError::MissingObject(id))?.parent;
        match parent {
            Some(p) => self.object_mut(p)?.children.retain(|c| *c != id),
            None => {
                self.touch_roots();
                Arc::make_mut(&mut self.doc.roots).retain(|c| *c != id);
            }
        }
        Ok(())
    }

    /// Delete an object and everything inside it.
    pub fn remove_object(&mut self, id: ObjectId) -> Result<(), DocError> {
        let subtree = self.doc.subtree(id);
        if subtree.is_empty() {
            return Err(DocError::MissingObject(id));
        }
        self.unlink(id)?;
        for i in subtree {
            self.touch_object(i);
            self.doc.objects.remove(&i);
        }
        Ok(())
    }

    /// Move an object to a new parent (or the top level). With `keep_world`, it stays put on screen.
    pub fn reparent(
        &mut self,
        id: ObjectId,
        new_parent: Option<ObjectId>,
        index: Option<usize>,
        keep_world: bool,
    ) -> Result<(), DocError> {
        if !self.doc.contains(id) {
            return Err(DocError::MissingObject(id));
        }
        if let Some(p) = new_parent {
            if !self.doc.contains(p) {
                return Err(DocError::MissingObject(p));
            }
            if p == id || self.doc.is_ancestor(id, p) {
                return Err(DocError::Cycle(id));
            }
        }
        let world = self.doc.world_transform(id);
        self.unlink(id)?;
        self.object_mut(id)?.parent = new_parent;
        self.link(id, new_parent, index)?;
        if keep_world {
            let parent_world = new_parent.map(|p| self.doc.world_transform(p)).unwrap_or(Transform::IDENTITY);
            self.object_mut(id)?.transform = parent_world.inverse().mul(&world);
        }
        Ok(())
    }

    /// Editable mesh of a shape. A shape that still has sliders is "locked in"
    /// (turned into a plain mesh) as part of this same step.
    pub fn mesh_mut(&mut self, id: ObjectId) -> Result<&mut PolyMesh, DocError> {
        let obj = self.object_mut(id)?;
        let shape = obj.shape_mut().ok_or(DocError::NotAShape(id))?;
        if let ShapeSource::Primitive(p) = &shape.source {
            shape.source = ShapeSource::Mesh(Arc::new(p.build()));
        }
        match &mut shape.source {
            ShapeSource::Mesh(m) => Ok(Arc::make_mut(m)),
            ShapeSource::Primitive(_) => unreachable!(),
        }
    }

    pub fn add_material(&mut self, m: Material) -> MaterialId {
        let id = MaterialId(self.doc.next_material);
        self.doc.next_material += 1;
        self.touch_material(id);
        self.doc.materials.insert(id, Arc::new(m));
        id
    }

    pub fn material_mut(&mut self, id: MaterialId) -> Result<&mut Material, DocError> {
        if !self.doc.materials.contains_key(&id) {
            return Err(DocError::MissingMaterial(id));
        }
        self.touch_material(id);
        Ok(Arc::make_mut(self.doc.materials.get_mut(&id).expect("checked above")))
    }

    /// Remove a swatch nothing uses.
    pub fn remove_material(&mut self, id: MaterialId) -> Result<(), DocError> {
        if !self.doc.materials.contains_key(&id) {
            return Err(DocError::MissingMaterial(id));
        }
        if self.doc.material_users(id) > 0 {
            return Err(DocError::Invalid("That color is still used.".into()));
        }
        self.touch_material(id);
        self.doc.materials.remove(&id);
        Ok(())
    }

    pub fn meta_mut(&mut self) -> &mut DocMeta {
        if self.before.meta.is_none() {
            self.before.meta = Some(self.doc.meta.clone());
        }
        Arc::make_mut(&mut self.doc.meta)
    }

    pub fn scene_mut(&mut self) -> &mut SceneSettings {
        if self.before.scene.is_none() {
            self.before.scene = Some(self.doc.scene.clone());
        }
        Arc::make_mut(&mut self.doc.scene)
    }

    /// Finish. Returns `None` if nothing actually changed (nothing is recorded).
    pub fn commit(mut self) -> Option<DocDelta> {
        self.done = true;
        let before = std::mem::take(&mut self.before);
        let mut after = Snapshot::default();
        let mut kept = Snapshot::default();

        // Unchanged parts get their original `Arc` back, so pointer-keyed caches stay valid.
        if let Some(b) = before.meta {
            if same(&b, &self.doc.meta) {
                self.doc.meta = b;
            } else {
                after.meta = Some(self.doc.meta.clone());
                kept.meta = Some(b);
            }
        }
        if let Some(b) = before.scene {
            if same(&b, &self.doc.scene) {
                self.doc.scene = b;
            } else {
                after.scene = Some(self.doc.scene.clone());
                kept.scene = Some(b);
            }
        }
        if let Some(b) = before.roots {
            if same(&b, &self.doc.roots) {
                self.doc.roots = b;
            } else {
                after.roots = Some(self.doc.roots.clone());
                kept.roots = Some(b);
            }
        }
        for (id, b) in before.objects {
            let a = self.doc.objects.get(&id).cloned();
            if same_opt(&b, &a) {
                if let Some(b) = b {
                    self.doc.objects.insert(id, b);
                }
            } else {
                after.objects.insert(id, a);
                kept.objects.insert(id, b);
            }
        }
        for (id, b) in before.materials {
            let a = self.doc.materials.get(&id).cloned();
            if same_opt(&b, &a) {
                if let Some(b) = b {
                    self.doc.materials.insert(id, b);
                }
            } else {
                after.materials.insert(id, a);
                kept.materials.insert(id, b);
            }
        }
        if kept.is_empty() { None } else { Some(DocDelta { before: kept, after }) }
    }

    /// Throw the changes away.
    pub fn abort(mut self) {
        self.rollback();
    }

    fn rollback(&mut self) {
        if !self.done {
            self.done = true;
            let before = std::mem::take(&mut self.before);
            before.restore_into(self.doc);
        }
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        self.rollback();
    }
}

fn same<T: PartialEq>(a: &Arc<T>, b: &Arc<T>) -> bool {
    Arc::ptr_eq(a, b) || **a == **b
}

fn same_opt<T: PartialEq>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same(a, b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Shape;
    use ss_mesh::Primitive;

    fn block(doc: &Document) -> Object {
        Object::new(
            "Block 1",
            ObjectKind::Shape(Shape::new(
                ShapeSource::Primitive(Primitive::default_block()),
                doc.default_material().unwrap(),
            )),
        )
    }

    #[test]
    fn commit_undo_redo_roundtrip() {
        let mut doc = Document::default();
        let start = doc.clone();
        let mut tx = doc.begin("Add Block");
        let obj = block(tx.doc());
        let id = tx.add_object(obj, None, None).unwrap();
        let delta = tx.commit().unwrap();
        let end = doc.clone();
        assert_eq!(doc.roots(), &[id]);
        delta.apply_before(&mut doc);
        assert_eq!(doc, start.clone_with_counters(&doc));
        delta.apply_after(&mut doc);
        assert_eq!(doc, end);
        assert!(doc.check_invariants().is_empty());
    }

    #[test]
    fn dropped_tx_rolls_back_and_noop_commit_records_nothing() {
        let mut doc = Document::default();
        let mut tx = doc.begin("Add");
        let obj = block(tx.doc());
        let id = tx.add_object(obj, None, None).unwrap();
        tx.commit().unwrap();
        let before = doc.clone();
        {
            let mut tx = doc.begin("Move");
            tx.object_mut(id).unwrap().transform.translation.x = 5.0;
        }
        assert_eq!(doc, before);
        let mut tx = doc.begin("Nothing");
        let _ = tx.object_mut(id).unwrap();
        assert!(tx.commit().is_none());
        assert!(Arc::ptr_eq(doc.object_arc(id).unwrap(), before.object_arc(id).unwrap()));
    }

    #[test]
    fn mesh_mut_locks_in_primitive_in_same_step() {
        let mut doc = Document::default();
        let mut tx = doc.begin("Add");
        let obj = block(tx.doc());
        let id = tx.add_object(obj, None, None).unwrap();
        tx.commit();
        let mut tx = doc.begin("Edit");
        let m = tx.mesh_mut(id).unwrap();
        assert_eq!(m.face_count(), 6);
        let d = tx.commit().unwrap();
        assert!(matches!(doc.object(id).unwrap().shape().unwrap().source, ShapeSource::Mesh(_)));
        d.apply_before(&mut doc);
        assert!(matches!(doc.object(id).unwrap().shape().unwrap().source, ShapeSource::Primitive(_)));
    }

    #[test]
    fn reparent_keeps_world_and_rejects_cycles() {
        let mut doc = Document::default();
        let mut tx = doc.begin("Setup");
        let a = tx.add_object(Object::new("A", ObjectKind::Group(Default::default())), None, None).unwrap();
        let obj = block(tx.doc());
        let b = tx.add_object(obj, None, None).unwrap();
        tx.object_mut(a).unwrap().transform.translation = glam::Vec3::new(1.0, 2.0, 3.0);
        tx.object_mut(b).unwrap().transform.translation = glam::Vec3::new(5.0, 0.0, 0.0);
        tx.reparent(b, Some(a), None, true).unwrap();
        assert!(matches!(tx.reparent(a, Some(b), None, true), Err(DocError::Cycle(_))));
        tx.commit();
        assert_eq!(doc.world_transform(b).translation, glam::Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(doc.children(a), &[b]);
        assert!(doc.check_invariants().is_empty(), "{:?}", doc.check_invariants());
    }

    impl Document {
        /// Test helper: compare documents ignoring the never-reused id counters.
        fn clone_with_counters(mut self, other: &Document) -> Document {
            self.next_object = other.next_object;
            self.next_material = other.next_material;
            self
        }
    }
}
