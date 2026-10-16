//! Shapeshop's editable polygon mesh.
//!
//! - [`PolyMesh`]: points + n-gon faces with stable ids ([`VertId`], [`FaceId`]).
//! - [`primitives`]: the starter shapes with fixed face ids.
//! - Derived data, cached per revision: [`Topology`], [`Triangulation`] (the one
//!   triangulation every consumer uses), [`Bvh`] for picking, [`MeshHealth`].
//! - [`RenderMesh`]: GPU-ready triangles (still pure CPU).
//!
//! This crate never depends on any UI or GPU crate.

pub mod bvh;
pub mod health;
pub mod ids;
pub mod mesh;
pub mod primitives;
pub mod render;
pub mod store;
pub mod topology;
pub mod triangulate;

pub use bvh::{Bvh, MeshHit};
pub use health::MeshHealth;
pub use ids::{EdgeKey, FaceId, MaterialId, VertId};
pub use mesh::{EdgeAttrs, Face, FaceVerts, PolyMesh, next_rev};
pub use primitives::{Primitive, PrimitiveKind};
pub use render::{RenderMesh, Shading, VertexMaterial};
pub use store::{ElementStore, StoreId};
pub use topology::Topology;
pub use triangulate::Triangulation;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum MeshError {
    #[error("point {0} doesn't exist")]
    MissingVert(VertId),
    #[error("face {0} doesn't exist")]
    MissingFace(FaceId),
    #[error("a face needs at least 3 points (got {0})")]
    TooFewVerts(usize),
    #[error("point {0} appears twice in one face")]
    RepeatedVert(VertId),
    #[error("point {0} is still used by a face")]
    VertInUse(VertId),
    #[error("{0}")]
    Invalid(String),
}
