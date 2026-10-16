//! Everything the user can ask the app to do, as plain data.
//!
//! Buttons, keyboard shortcuts, the search box, quests ("Do it for me"),
//! "Why is this grey? [Fix]" buttons and scenario scripts all produce an
//! [`AppAction`]; the app has exactly one `dispatch` that performs it.
//! Being plain serializable data, actions can be written in `tools.ron`,
//! `quests.ron` and test scripts.

use serde::{Deserialize, Serialize};
use ss_doc::{ObjectId, ParamValues, PickFilter};
use ss_math::CameraView;

/// What a left-drag in the viewport does (the Tool strip).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActiveTool {
    #[default]
    Select,
    Move,
    /// Roblox-style one-sided size handles.
    Size,
    Turn,
}

impl ActiveTool {
    pub const ALL: [ActiveTool; 4] = [ActiveTool::Select, ActiveTool::Move, ActiveTool::Size, ActiveTool::Turn];

    pub fn label(self) -> &'static str {
        match self {
            ActiveTool::Select => "Select",
            ActiveTool::Move => "Move",
            ActiveTool::Size => "Size",
            ActiveTool::Turn => "Turn",
        }
    }

    /// The Blender name, shown second in tips.
    pub fn term(self) -> &'static str {
        match self {
            ActiveTool::Select => "Select",
            ActiveTool::Move => "Grab",
            ActiveTool::Size => "Scale",
            ActiveTool::Turn => "Rotate",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SelectOp {
    None,
    All,
    Only(ObjectId),
    Toggle(ObjectId),
    /// First object whose name matches exactly (used by scripts and quests).
    ByName(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CameraOp {
    Home,
    /// Fit the selection (or everything) in view.
    Frame,
    View(CameraView),
    /// Radians.
    Orbit {
        yaw: f32,
        pitch: f32,
    },
    /// Factor > 1 zooms in.
    Zoom(f32),
    ToggleFlat,
    ToggleXRay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FileOp {
    New,
    Open,
    OpenPath(std::path::PathBuf),
    Save,
    SaveAs,
    /// Index into the recent-files list.
    OpenRecent(usize),
    ShowInFolder,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GuideOp {
    OpenSearch,
    StartQuest(String),
    SkipQuestStep,
    ExitQuest,
    ShowArticle(String),
    /// Spotlight the place where a tool lives.
    ShowMe(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AppAction {
    /// Run an operator now. Missing params take the tool's defaults.
    RunTool {
        id: String,
        params: ParamValues,
    },
    /// Start an interactive tool (e.g. Push/Pull waits for a drag).
    StartTool(String),
    SetTool(ActiveTool),
    SetFilter(PickFilter),
    Select(SelectOp),
    Undo,
    Redo,
    /// Go to the state after this many History steps.
    JumpHistory(usize),
    Camera(CameraOp),
    File(FileOp),
    /// Export with a preset key ("roblox", "unity", "godot", "print", "other").
    Export {
        preset: String,
    },
    ExportAgain,
    TakePicture,
    Guide(GuideOp),
    /// Change a setting by key ("theme", "ui_scale", "controls", "reduce_motion", ...).
    SetSetting {
        key: String,
        value: String,
    },
    OpenSettings,
}

impl AppAction {
    pub fn run(id: impl Into<String>) -> Self {
        AppAction::RunTool { id: id.into(), params: ParamValues::new() }
    }
}
