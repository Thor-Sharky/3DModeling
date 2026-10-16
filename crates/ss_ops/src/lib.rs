//! Tools as pure operators, plus the data-driven tool registry.
//!
//! - [`AppAction`]: everything the user can ask for, as data.
//! - [`ToolDef`]: labels, tips, reasons, params (from `assets/guide/tools.ron`).
//! - [`Operator`]: `poll` (can it run? if not, why?) and `execute` (inside a `Tx`).
//! - [`Registry`]: every operator paired with its `ToolDef`; [`run_tool`] runs one
//!   and returns the history entry.
//!
//! No UI code lives here: operators only see the document, the selection and a
//! small [`OpContext`].

pub mod action;
pub mod tooldef;

pub use action::{ActiveTool, AppAction, CameraOp, FileOp, GuideOp, SelectOp};
pub use tooldef::{Demo, Needs, ParamDef, ParamKind, Reason, ToolDef};

use glam::Vec3;
use ss_doc::{Document, ParamValues, Selection, Tx};

/// What operators may know about the app besides the document.
#[derive(Clone, Debug, Default)]
pub struct OpContext {
    pub selection: Selection,
    /// Where new shapes go when no position is given (the camera focus on the ground).
    pub place_at: Option<Vec3>,
    /// Snapping enabled (Shift held = off).
    pub snap: bool,
}

/// Can the tool run now?
#[derive(Clone, Debug, PartialEq)]
pub enum Availability {
    Ready,
    Unavailable { reason: Reason },
}

/// What happened, besides the document change.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpOutcome {
    /// New selection after the operation (`None` keeps the current one).
    pub selection: Option<Selection>,
    /// A short toast, e.g. "Locked in so you can edit faces. Undo brings the sliders back."
    pub message: Option<String>,
}

/// An error a person can understand, with an optional fix button.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{title}: {message}")]
pub struct FriendlyError {
    pub title: String,
    pub message: String,
    pub fix: Option<AppAction>,
    /// Technical details for bug reports (never shown first).
    pub details: String,
}

impl FriendlyError {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self { title: title.into(), message: message.into(), fix: None, details: String::new() }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = details.into();
        self
    }

    pub fn with_fix(mut self, fix: AppAction) -> Self {
        self.fix = Some(fix);
        self
    }
}

impl From<ss_doc::DocError> for FriendlyError {
    fn from(e: ss_doc::DocError) -> Self {
        FriendlyError::new("That didn't work", "Something in your scene changed before this could finish.")
            .with_details(e.to_string())
    }
}

pub trait Operator: Send + Sync {
    fn id(&self) -> &str;
    fn poll(&self, doc: &Document, cx: &OpContext) -> Availability;
    fn execute(&self, tx: &mut Tx<'_>, cx: &OpContext, params: &ParamValues) -> Result<OpOutcome, FriendlyError>;
}
