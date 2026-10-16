//! What is picked. Not part of the document (selection changes are not edits),
//! but every history entry remembers the selection before and after.

use std::collections::BTreeSet;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use ss_mesh::{EdgeKey, FaceId, VertId};

use crate::{Document, ObjectId};

/// What a click grabs. Replaces Blender's Object/Edit modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PickFilter {
    #[default]
    Objects,
    Faces,
    Edges,
    Points,
}

impl PickFilter {
    pub const ALL: [PickFilter; 4] = [PickFilter::Objects, PickFilter::Faces, PickFilter::Edges, PickFilter::Points];

    pub fn label(self) -> &'static str {
        match self {
            PickFilter::Objects => "Objects",
            PickFilter::Faces => "Faces",
            PickFilter::Edges => "Edges",
            PickFilter::Points => "Points",
        }
    }
}

/// Faces/edges/points picked on one object.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ElementSel {
    pub object: Option<ObjectId>,
    pub faces: BTreeSet<FaceId>,
    pub edges: BTreeSet<EdgeKey>,
    pub verts: BTreeSet<VertId>,
}

impl ElementSel {
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty() && self.edges.is_empty() && self.verts.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub filter: PickFilter,
    /// Picked objects; the last one is the "active" object.
    pub objects: IndexSet<ObjectId>,
    pub elements: ElementSel,
}

impl Selection {
    pub fn single(id: ObjectId) -> Self {
        let mut s = Self::default();
        s.objects.insert(id);
        s
    }

    pub fn active(&self) -> Option<ObjectId> {
        self.objects.last().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.elements.is_empty()
    }

    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects.contains(&id)
    }

    pub fn clear(&mut self) {
        self.objects.clear();
        self.elements = ElementSel::default();
    }

    pub fn set_only(&mut self, id: ObjectId) {
        self.objects.clear();
        self.objects.insert(id);
        self.elements = ElementSel::default();
    }

    /// Shift-click: add, or remove if already picked.
    pub fn toggle(&mut self, id: ObjectId) {
        if !self.objects.shift_remove(&id) {
            self.objects.insert(id);
        }
    }

    /// Drop anything that no longer exists in `doc`.
    pub fn validate(&mut self, doc: &Document) {
        self.objects.retain(|id| doc.contains(*id));
        if let Some(obj) = self.elements.object {
            match doc.object(obj).and_then(|o| o.shape()) {
                Some(_) => {
                    if let Some(mesh) = doc.mesh_of(obj) {
                        self.elements.faces.retain(|f| mesh.contains_face(*f));
                        self.elements.verts.retain(|v| mesh.contains_vert(*v));
                        self.elements.edges.retain(|e| mesh.contains_vert(e.a) && mesh.contains_vert(e.b));
                    }
                }
                None => self.elements = ElementSel::default(),
            }
        }
    }
}
