//! Evaluation: turn shapes (sliders, edited meshes, effects) into final meshes.
//!
//! Results are cached per object by content key, keeping the last few results
//! so undo/redo is instant. Evaluation is synchronous today; the
//! request/poll shape of the API leaves room for a worker thread later.

use std::collections::BTreeMap;
use std::sync::Arc;

use ss_mesh::{PolyMesh, Primitive};

use crate::object::{Effects, ShapeSource};
use crate::{Document, ObjectId};

const KEEP_PER_OBJECT: usize = 3;

#[derive(Clone, Debug, PartialEq)]
enum SourceKey {
    Prim(Primitive),
    Mesh(u64),
}

#[derive(Clone, Debug, PartialEq)]
struct Key {
    source: SourceKey,
    pivot: glam::Vec3,
    effects: Effects,
}

#[derive(Default)]
pub struct Evaluator {
    cache: BTreeMap<ObjectId, Vec<(Key, Arc<PolyMesh>)>>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Final mesh of a shape in its own object space (effects applied).
    /// `None` for objects that aren't shapes.
    pub fn shape_mesh(&mut self, doc: &Document, id: ObjectId) -> Option<Arc<PolyMesh>> {
        let shape = doc.object(id)?.shape()?;
        let key = Key {
            source: match &shape.source {
                ShapeSource::Primitive(p) => SourceKey::Prim(p.clone()),
                ShapeSource::Mesh(m) => SourceKey::Mesh(m.rev()),
            },
            pivot: shape.pivot,
            effects: shape.effects.clone(),
        };
        let slot = self.cache.entry(id).or_default();
        if let Some(pos) = slot.iter().position(|(k, _)| *k == key) {
            let hit = slot.remove(pos);
            let mesh = hit.1.clone();
            slot.push(hit);
            return Some(mesh);
        }
        let base: Arc<PolyMesh> = shape.base_mesh();
        // Live effects (Mirror, Repeat, Smooth) are applied here in milestone 6.
        let mesh = base;
        slot.push((key, mesh.clone()));
        if slot.len() > KEEP_PER_OBJECT {
            slot.remove(0);
        }
        Some(mesh)
    }

    /// Forget objects that no longer exist.
    pub fn retain_existing(&mut self, doc: &Document) {
        self.cache.retain(|id, _| doc.contains(*id));
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectKind, Shape};

    #[test]
    fn cache_hits_and_follows_edits() {
        let mut doc = Document::default();
        let mat = doc.default_material().unwrap();
        let mut tx = doc.begin("Add");
        let id = tx
            .add_object(
                Object::new(
                    "B",
                    ObjectKind::Shape(Shape::new(ShapeSource::Primitive(Primitive::default_block()), mat)),
                ),
                None,
                None,
            )
            .unwrap();
        tx.commit();
        let mut ev = Evaluator::new();
        let a = ev.shape_mesh(&doc, id).unwrap();
        let b = ev.shape_mesh(&doc, id).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let mut tx = doc.begin("Size");
        if let ShapeSource::Primitive(p) = &mut tx.object_mut(id).unwrap().shape_mut().unwrap().source {
            p.set_size(glam::Vec3::splat(3.0));
        }
        let d = tx.commit().unwrap();
        let c = ev.shape_mesh(&doc, id).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        d.apply_before(&mut doc);
        let again = ev.shape_mesh(&doc, id).unwrap();
        assert!(Arc::ptr_eq(&a, &again), "undo should hit the cache");
    }
}
