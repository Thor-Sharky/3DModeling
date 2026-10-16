//! Stable element ids. Ids are never reused within a mesh, so selections,
//! colors and history stay valid across edits.

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl From<u32> for $name {
            fn from(v: u32) -> Self {
                Self(v)
            }
        }

        impl crate::store::StoreId for $name {
            fn from_raw(raw: u32) -> Self {
                Self(raw)
            }
            fn raw(self) -> u32 {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}{}", stringify!($name).trim_end_matches("Id").chars().next().unwrap(), self.0)
            }
        }
    };
}

id_type!(
    /// A point (vertex) of a mesh.
    VertId
);
id_type!(
    /// A face (polygon) of a mesh.
    FaceId
);
id_type!(
    /// A palette swatch / material in the document.
    MaterialId
);

/// An undirected edge, always stored with `a < b`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeKey {
    pub a: VertId,
    pub b: VertId,
}

impl EdgeKey {
    pub fn new(x: VertId, y: VertId) -> Self {
        if x <= y { Self { a: x, b: y } } else { Self { a: y, b: x } }
    }

    pub fn contains(&self, v: VertId) -> bool {
        self.a == v || self.b == v
    }

    pub fn other(&self, v: VertId) -> Option<VertId> {
        if self.a == v {
            Some(self.b)
        } else if self.b == v {
            Some(self.a)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_key_is_normalized() {
        assert_eq!(EdgeKey::new(VertId(5), VertId(2)), EdgeKey::new(VertId(2), VertId(5)));
        assert_eq!(EdgeKey::new(VertId(5), VertId(2)).a, VertId(2));
        assert_eq!(EdgeKey::new(VertId(1), VertId(2)).other(VertId(2)), Some(VertId(1)));
        assert_eq!(format!("{}", FaceId(3)), "F3");
    }
}
