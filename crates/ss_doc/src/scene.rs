//! Project-wide settings: what the project is for, units, grid, and the scene look.

use serde::{Deserialize, Serialize};
use ss_math::Units;

/// What the user said they're making on the Welcome card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProjectTarget {
    #[default]
    Roblox,
    GameAsset,
    ArtScene,
}

impl ProjectTarget {
    pub fn label(self) -> &'static str {
        match self {
            ProjectTarget::Roblox => "Roblox thing",
            ProjectTarget::GameAsset => "Game asset",
            ProjectTarget::ArtScene => "Art scene",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocMeta {
    pub title: String,
    pub target: ProjectTarget,
    pub units: Units,
    /// Grid line spacing.
    pub grid_step: f32,
    pub snap: SnapSettings,
    /// Triangle limit per exported part (Roblox: 20,000).
    pub tri_budget: u32,
}

impl DocMeta {
    pub fn for_target(target: ProjectTarget) -> Self {
        match target {
            ProjectTarget::Roblox => Self {
                title: "Untitled".into(),
                target,
                units: Units::Studs,
                grid_step: 1.0,
                snap: SnapSettings::default(),
                tri_budget: 20_000,
            },
            ProjectTarget::GameAsset => Self {
                title: "Untitled".into(),
                target,
                units: Units::Meters,
                grid_step: 0.5,
                snap: SnapSettings { enabled: true, move_step: 0.25, turn_deg: 15.0, size_step: 0.25 },
                tri_budget: 20_000,
            },
            ProjectTarget::ArtScene => Self {
                title: "Untitled".into(),
                target,
                units: Units::Meters,
                grid_step: 1.0,
                snap: SnapSettings { enabled: false, move_step: 0.25, turn_deg: 15.0, size_step: 0.25 },
                tri_budget: 200_000,
            },
        }
    }

    /// Smallest allowed size for any part (Roblox simulates anything smaller as 0.05 studs).
    pub fn min_part_size(&self) -> f32 {
        ss_mesh::primitives::MIN_SIZE
    }
}

impl Default for DocMeta {
    fn default() -> Self {
        Self::for_target(ProjectTarget::Roblox)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapSettings {
    pub enabled: bool,
    pub move_step: f32,
    pub turn_deg: f32,
    pub size_step: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self { enabled: true, move_step: 1.0, turn_deg: 15.0, size_step: 1.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LightPreset {
    #[default]
    SunnyDay,
    SoftStudio,
    Sunset,
    NightNeon,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneSettings {
    pub preset: LightPreset,
    /// Hours, 0..24. Moves the sun.
    pub time_of_day: f32,
    /// sRGB 0..1.
    pub sky_top: [f32; 3],
    pub sky_bottom: [f32; 3],
    pub sun_brightness: f32,
    pub shadows: bool,
    /// A ground plane that catches shadows in pictures.
    pub ground: bool,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            preset: LightPreset::SunnyDay,
            time_of_day: 14.0,
            sky_top: [0.53, 0.70, 0.92],
            sky_bottom: [0.86, 0.90, 0.95],
            sun_brightness: 1.0,
            shadows: true,
            ground: true,
        }
    }
}
