//! Tool metadata: the single source for button labels, hover tips, hints,
//! disabled reasons, search keywords and parameter UIs. Loaded from
//! `assets/guide/tools.ron` (embedded in the binary).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ss_doc::{ParamValue, PickFilter};

use crate::AppAction;

/// What a tool needs before it can run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Needs {
    #[default]
    Nothing,
    /// At least `min` picked objects of any kind.
    Objects {
        min: usize,
    },
    /// At least `min` picked shapes.
    Shapes {
        min: usize,
    },
    /// Faces picked on one shape.
    Faces {
        min: usize,
    },
    Edges {
        min: usize,
    },
    Points {
        min: usize,
    },
}

/// Machine-readable reasons a tool can't run right now. Each has friendly text
/// (and optionally a fix) in the tool's `reasons` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Reason {
    NeedObjects,
    NeedShapes,
    NeedTwoShapes,
    NeedFaces,
    NeedEdges,
    NeedPoints,
    Locked,
    NothingToUndo,
    NotAvailableYet,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParamKind {
    /// A length in scene units (studs or meters), with min/max.
    Length(f32, f32),
    /// Degrees.
    Angle(f32, f32),
    /// Plain number with min/max.
    Number(f32, f32),
    Integer(i64, i64),
    Toggle,
    /// (key, label) options.
    Choice(Vec<(String, String)>),
    Vector3,
    Text,
    Color,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParamDef {
    pub key: String,
    pub label: String,
    pub kind: ParamKind,
    pub default: ParamValue,
    #[serde(default)]
    pub tip: Option<String>,
}

/// A before/after demo for "Show me".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Demo {
    /// Starting shape key, e.g. "block".
    pub start: String,
    #[serde(default)]
    pub select_faces: Vec<u32>,
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    /// Stable id, e.g. "mesh.extrude", "shape.add.block".
    pub id: String,
    /// Friendly name shown on the button, e.g. "Push / Pull".
    pub name: String,
    /// The usual 3D term, shown second: "Extrude". The only place jargon is allowed.
    #[serde(default)]
    pub term: Option<String>,
    /// egui-phosphor icon constant name, e.g. "ARROWS_OUT_LINE_VERTICAL".
    #[serde(default)]
    pub icon: Option<String>,
    /// Where it lives, e.g. "Left panel > Shapes".
    pub location: String,
    #[serde(default)]
    pub needs: Needs,
    /// Switch the Pick filter to this when the tool starts.
    #[serde(default)]
    pub auto_filter: Option<PickFilter>,
    /// One plain sentence (<= 140 characters).
    pub tip: String,
    /// Shown in the hint bar while the tool is active.
    pub hint: String,
    /// Search words (>= 3).
    pub keywords: Vec<String>,
    /// Goal phrases for the search box: "make a hole".
    #[serde(default)]
    pub intents: Vec<String>,
    /// Shortcut per controls preset ("roblox", "trackpad", "blender").
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
    /// Name of the interactive handler if the tool needs a drag (e.g. "push_pull").
    #[serde(default)]
    pub interactive: Option<String>,
    /// Friendly text and an optional fix for each reason it can be unavailable.
    #[serde(default)]
    pub reasons: BTreeMap<Reason, (String, Option<AppAction>)>,
    #[serde(default)]
    pub params: Vec<ParamDef>,
    #[serde(default)]
    pub demo: Option<Demo>,
    /// Hidden from panels and search (internal tools used by gizmos and scripts).
    #[serde(default)]
    pub internal: bool,
}

impl ToolDef {
    pub fn default_params(&self) -> ss_doc::ParamValues {
        self.params.iter().map(|p| (p.key.clone(), p.default.clone())).collect()
    }

    /// "Push / Pull (Extrude)".
    pub fn display_name(&self) -> String {
        match &self.term {
            Some(t) if t != &self.name => format!("{} ({t})", self.name),
            _ => self.name.clone(),
        }
    }
}
