//! Mesh booleans behind a swappable engine.
//!
//! Milestone 5 adds the Manifold engine (`manifold-csg`, behind the default-on
//! `manifold` feature). Until then only the trait and the friendly errors exist,
//! so the rest of the app can already be written against them.

use ss_mesh::PolyMesh;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BooleanOp {
    /// Solids merged, cutters removed.
    Union,
    /// Keep only the overlap.
    Intersect,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum CsgError {
    #[error("Booleans aren't in this build of Shapeshop.")]
    Unavailable,
    #[error("This shape has a gap, so it can't be used to cut or join.")]
    NotSolid { input: usize },
    #[error("The building-block step failed: {0}")]
    Engine(String),
}

/// One input to a boolean: a mesh already placed in the group's space.
pub struct CsgInput<'a> {
    pub mesh: &'a PolyMesh,
    /// Cutters are subtracted; solids are joined.
    pub cutter: bool,
}

pub trait BooleanEngine: Send + Sync {
    fn name(&self) -> &'static str;
    /// `union(solids) - union(cutters)` for [`BooleanOp::Union`], or the overlap of all solids
    /// (minus cutters) for [`BooleanOp::Intersect`]. Output faces keep their source colors.
    fn combine(&self, inputs: &[CsgInput<'_>], op: BooleanOp) -> Result<PolyMesh, CsgError>;
}

/// The engine used when booleans are not compiled in.
pub struct NoEngine;

impl BooleanEngine for NoEngine {
    fn name(&self) -> &'static str {
        "none"
    }

    fn combine(&self, _inputs: &[CsgInput<'_>], _op: BooleanOp) -> Result<PolyMesh, CsgError> {
        Err(CsgError::Unavailable)
    }
}

/// The best engine in this build.
pub fn default_engine() -> &'static dyn BooleanEngine {
    &NoEngine
}
