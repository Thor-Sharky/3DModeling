//! Undo/redo history. Every entry restores the selection it had.

use crate::Document;
use crate::params::Invocation;
use crate::selection::Selection;
use crate::tx::DocDelta;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub label: String,
    /// The tool and settings that made this step (for "Adjust last action").
    pub invocation: Option<Invocation>,
    pub delta: DocDelta,
    pub sel_before: Selection,
    pub sel_after: Selection,
    /// Consecutive entries with the same key merge into one step (slider drags).
    pub coalesce: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<HistoryEntry>,
    /// Number of entries currently applied.
    cursor: usize,
    /// `cursor` value when the file was last saved (`None` = never matches).
    saved_at: Option<usize>,
    budget_bytes: usize,
}

impl Default for History {
    fn default() -> Self {
        Self { entries: Vec::new(), cursor: 0, saved_at: Some(0), budget_bytes: 512 * 1024 * 1024 }
    }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_budget(budget_bytes: usize) -> Self {
        Self { budget_bytes, ..Self::default() }
    }

    /// Record a committed step. Anything that was undone is dropped.
    pub fn push(&mut self, entry: HistoryEntry) {
        self.entries.truncate(self.cursor);
        if self.saved_at.is_some_and(|s| s > self.cursor) {
            self.saved_at = None;
        }
        if let (Some(key), Some(last)) = (entry.coalesce, self.entries.last_mut())
            && last.coalesce == Some(key)
            && self.saved_at != Some(self.cursor)
        {
            let delta = std::mem::take(&mut last.delta);
            last.delta = delta.merge(entry.delta);
            last.sel_after = entry.sel_after;
            last.invocation = entry.invocation.or(last.invocation.take());
            return;
        }
        self.entries.push(entry);
        self.cursor = self.entries.len();
        self.enforce_budget();
    }

    fn enforce_budget(&mut self) {
        let mut total: usize = self.entries.iter().map(|e| e.delta.approx_bytes()).sum();
        while total > self.budget_bytes && self.entries.len() > 1 {
            let e = self.entries.remove(0);
            total -= e.delta.approx_bytes();
            self.cursor -= 1;
            self.saved_at = self.saved_at.and_then(|s| s.checked_sub(1));
        }
    }

    /// Undo one step. Returns the selection to restore.
    pub fn undo(&mut self, doc: &mut Document) -> Option<Selection> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        let e = &self.entries[self.cursor];
        e.delta.apply_before(doc);
        Some(e.sel_before.clone())
    }

    /// Redo one step. Returns the selection to restore.
    pub fn redo(&mut self, doc: &mut Document) -> Option<Selection> {
        let e = self.entries.get(self.cursor)?;
        e.delta.apply_after(doc);
        self.cursor += 1;
        Some(e.sel_after.clone())
    }

    /// Go to the state after `applied` entries (0 = the start). Clicking a row in the History list.
    pub fn jump_to(&mut self, applied: usize, doc: &mut Document) -> Option<Selection> {
        let target = applied.min(self.entries.len());
        let mut sel = None;
        while self.cursor > target {
            sel = self.undo(doc);
        }
        while self.cursor < target {
            sel = self.redo(doc);
        }
        sel
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.entries.len()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.cursor.checked_sub(1).map(|i| self.entries[i].label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.entries.get(self.cursor).map(|e| e.label.as_str())
    }

    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The most recent applied step (what the Adjust card edits).
    pub fn last_applied(&self) -> Option<&HistoryEntry> {
        self.cursor.checked_sub(1).map(|i| &self.entries[i])
    }

    /// Replace the most recent applied step (the Adjust card re-ran it with new settings).
    /// The caller must already have undone it and applied `entry.delta`.
    pub fn replace_last(&mut self, entry: HistoryEntry) {
        if self.cursor == 0 {
            self.push(entry);
            return;
        }
        self.entries.truncate(self.cursor);
        if self.saved_at == Some(self.cursor) {
            self.saved_at = None;
        }
        *self.entries.last_mut().expect("cursor > 0") = entry;
    }

    /// Unsaved changes?
    pub fn is_dirty(&self) -> bool {
        self.saved_at != Some(self.cursor)
    }

    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.cursor);
    }

    /// Mark as never saved (e.g. a recovered autosave).
    pub fn mark_unsaved(&mut self) {
        self.saved_at = None;
    }

    pub fn clear(&mut self) {
        *self = Self { budget_bytes: self.budget_bytes, ..Self::default() };
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectKind, Shape, ShapeSource};
    use ss_mesh::Primitive;

    fn add_block(doc: &mut Document, hist: &mut History) -> crate::ObjectId {
        let mat = doc.default_material().unwrap();
        let mut tx = doc.begin("Add Block");
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
        let delta = tx.commit().unwrap();
        hist.push(HistoryEntry {
            label: "Add Block".into(),
            invocation: None,
            delta,
            sel_before: Selection::default(),
            sel_after: Selection::single(id),
            coalesce: None,
        });
        id
    }

    #[test]
    fn undo_redo_and_dirty() {
        let mut doc = Document::default();
        let mut h = History::new();
        assert!(!h.is_dirty());
        let id = add_block(&mut doc, &mut h);
        assert!(h.is_dirty());
        h.mark_saved();
        assert!(!h.is_dirty());
        let sel = h.undo(&mut doc).unwrap();
        assert!(sel.is_empty());
        assert!(!doc.contains(id));
        assert!(h.is_dirty());
        let sel = h.redo(&mut doc).unwrap();
        assert_eq!(sel.active(), Some(id));
        assert!(doc.contains(id));
        assert!(!h.is_dirty());
    }

    #[test]
    fn coalesced_slider_drags_are_one_step() {
        let mut doc = Document::default();
        let mut h = History::new();
        let id = add_block(&mut doc, &mut h);
        for x in 1..=5 {
            let mut tx = doc.begin("Move");
            tx.object_mut(id).unwrap().transform.translation.x = x as f32;
            let delta = tx.commit().unwrap();
            h.push(HistoryEntry {
                label: "Move".into(),
                invocation: None,
                delta,
                sel_before: Selection::single(id),
                sel_after: Selection::single(id),
                coalesce: Some(42),
            });
        }
        assert_eq!(h.len(), 2);
        h.undo(&mut doc);
        assert_eq!(doc.object(id).unwrap().transform.translation.x, 0.0);
        h.redo(&mut doc);
        assert_eq!(doc.object(id).unwrap().transform.translation.x, 5.0);
    }

    #[test]
    fn new_edit_after_undo_drops_redo() {
        let mut doc = Document::default();
        let mut h = History::new();
        add_block(&mut doc, &mut h);
        add_block(&mut doc, &mut h);
        h.undo(&mut doc);
        assert!(h.can_redo());
        add_block(&mut doc, &mut h);
        assert!(!h.can_redo());
        assert_eq!(h.len(), 2);
        h.jump_to(0, &mut doc);
        assert_eq!(doc.object_count(), 0);
        h.jump_to(2, &mut doc);
        assert_eq!(doc.object_count(), 2);
    }
}
