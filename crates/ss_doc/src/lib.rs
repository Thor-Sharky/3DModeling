//! The Shapeshop document model.
//!
//! - [`Document`]: objects (shapes, groups, lights, cameras), the palette and settings.
//!   Fields are private: every change goes through a [`Tx`].
//! - [`Tx`] / [`DocDelta`] / [`History`]: undo by construction.
//! - [`Selection`] and [`PickFilter`] (Objects / Faces / Edges / Points instead of modes).
//! - [`Evaluator`]: final meshes with caching; [`RenderScene`]: what to draw; [`pick`].
//!
//! No UI or GPU code lives here.

pub mod document;
pub mod eval;
pub mod history;
pub mod material;
pub mod object;
pub mod params;
pub mod pick;
pub mod render_scene;
pub mod scene;
pub mod selection;
pub mod tx;

pub use document::Document;
pub use eval::Evaluator;
pub use history::{History, HistoryEntry};
pub use material::Material;
pub use object::{
    Axis, Combine, Effects, Group, Light, LightKind, MirrorFx, Object, ObjectId, ObjectKind, PhotoCamera, RepeatFx,
    Role, Shape, ShapeSource, SmoothFx,
};
pub use params::{Invocation, ParamValue, ParamValues};
pub use pick::{PickHit, pick_object};
pub use render_scene::{DrawItem, DrawStyle, Environment, GridSpec, Highlight, RenderScene, SceneOptions, ViewStyle};
pub use scene::{DocMeta, LightPreset, ProjectTarget, SceneSettings, SnapSettings};
pub use selection::{ElementSel, PickFilter, Selection};
pub use tx::{DocDelta, Tx};

pub use ss_mesh;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DocError {
    #[error("object {0} doesn't exist")]
    MissingObject(ObjectId),
    #[error("object {0} isn't a shape")]
    NotAShape(ObjectId),
    #[error("color {0:?} doesn't exist")]
    MissingMaterial(ss_mesh::MaterialId),
    #[error("can't put {0} inside itself")]
    Cycle(ObjectId),
    #[error(transparent)]
    Mesh(#[from] ss_mesh::MeshError),
    #[error("{0}")]
    Invalid(String),
}
