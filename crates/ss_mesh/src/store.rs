//! An ordered id -> value store. Iteration order is ascending id, which makes
//! every algorithm (and every saved file) deterministic.

use std::collections::BTreeMap;

pub trait StoreId: Copy + Ord {
    fn from_raw(raw: u32) -> Self;
    fn raw(self) -> u32;
}

#[derive(Clone, Debug, PartialEq)]
pub struct ElementStore<K: StoreId, V> {
    items: BTreeMap<K, V>,
    next: u32,
}

impl<K: StoreId, V> Default for ElementStore<K, V> {
    fn default() -> Self {
        Self { items: BTreeMap::new(), next: 0 }
    }
}

impl<K: StoreId, V> ElementStore<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert with a fresh id.
    pub fn insert(&mut self, value: V) -> K {
        let id = K::from_raw(self.next);
        self.next += 1;
        self.items.insert(id, value);
        id
    }

    /// Insert (or replace) with a known id, e.g. when loading a file or restoring.
    /// Future fresh ids are always larger than any id ever inserted.
    pub fn insert_with_id(&mut self, id: K, value: V) -> Option<V> {
        self.next = self.next.max(id.raw() + 1);
        self.items.insert(id, value)
    }

    pub fn remove(&mut self, id: K) -> Option<V> {
        self.items.remove(&id)
    }

    pub fn get(&self, id: K) -> Option<&V> {
        self.items.get(&id)
    }

    pub fn get_mut(&mut self, id: K) -> Option<&mut V> {
        self.items.get_mut(&id)
    }

    pub fn contains(&self, id: K) -> bool {
        self.items.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (K, &V)> + ExactSizeIterator {
        self.items.iter().map(|(k, v)| (*k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut V)> {
        self.items.iter_mut().map(|(k, v)| (*k, v))
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = K> + ExactSizeIterator + '_ {
        self.items.keys().copied()
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> + ExactSizeIterator {
        self.items.values()
    }

    /// The id the next `insert` will use.
    pub fn next_id(&self) -> u32 {
        self.next
    }

    /// Raise the next fresh id (used when restoring a saved counter).
    pub fn bump_next_id(&mut self, next: u32) {
        self.next = self.next.max(next);
    }

    pub fn retain(&mut self, mut f: impl FnMut(K, &mut V) -> bool) {
        self.items.retain(|k, v| f(*k, v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VertId;

    #[test]
    fn ids_are_never_reused() {
        let mut s: ElementStore<VertId, i32> = ElementStore::new();
        let a = s.insert(1);
        let b = s.insert(2);
        s.remove(b);
        let c = s.insert(3);
        assert_eq!(a, VertId(0));
        assert_eq!(c, VertId(2));
        s.insert_with_id(VertId(10), 4);
        assert_eq!(s.insert(5), VertId(11));
        assert_eq!(s.keys().collect::<Vec<_>>(), vec![VertId(0), VertId(2), VertId(10), VertId(11)]);
    }
}
