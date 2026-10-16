//! Objects in the scene: shapes, groups, lights and photo cameras.

use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};
use ss_math::Transform;
use ss_mesh::{MaterialId, PolyMesh, Primitive, Shading};

/// An object in the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u32);

impl std::fmt::Display for ObjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub name: String,
    pub parent: Option<ObjectId>,
    /// Display and evaluation order.
    pub children: Vec<ObjectId>,
    /// Relative to the parent. Always rigid: sizing changes the shape, not a scale.
    pub transform: Transform,
    pub visible: bool,
    /// Locked objects can't be picked in the viewport.
    pub locked: bool,
    pub kind: ObjectKind,
}

impl Object {
    pub fn new(name: impl Into<String>, kind: ObjectKind) -> Self {
        Self {
            name: name.into(),
            parent: None,
            children: Vec::new(),
            transform: Transform::IDENTITY,
            visible: true,
            locked: false,
            kind,
        }
    }

    pub fn shape(&self) -> Option<&Shape> {
        match &self.kind {
            ObjectKind::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn shape_mut(&mut self) -> Option<&mut Shape> {
        match &mut self.kind {
            ObjectKind::Shape(s) => Some(s),
            _ => None,
        }
    }

    pub fn group(&self) -> Option<&Group> {
        match &self.kind {
            ObjectKind::Group(g) => Some(g),
            _ => None,
        }
    }

    pub fn is_cutter(&self) -> bool {
        self.shape().is_some_and(|s| s.role == Role::Cutter)
    }

    /// Short description for the outliner and tips, e.g. "Block", "Shape", "Union".
    pub fn kind_label(&self) -> &'static str {
        match &self.kind {
            ObjectKind::Shape(s) => match &s.source {
                ShapeSource::Primitive(p) => p.kind().label(),
                ShapeSource::Mesh(_) => "Shape",
            },
            ObjectKind::Group(g) => match g.combine {
                Combine::None => "Group",
                Combine::Union => "Union",
                Combine::Intersect => "Intersect",
            },
            ObjectKind::Light(l) => match l.kind {
                LightKind::Point => "Lamp",
                LightKind::Spot { .. } => "Spotlight",
            },
            ObjectKind::Camera(_) => "Photo Camera",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjectKind {
    Shape(Shape),
    Group(Group),
    Light(Light),
    Camera(PhotoCamera),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    #[default]
    Solid,
    /// Roblox "Negate": cuts away from solids in the same Union.
    Cutter,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub source: ShapeSource,
    pub role: Role,
    /// Used by faces without their own material.
    pub material: MaterialId,
    pub look: Shading,
    pub effects: Effects,
    /// The point of the source mesh (in its own coordinates) that sits at the
    /// object's origin. Primitives are built centered, so `ZERO` = pivot at the
    /// center; "Pivot: bottom" sets it to the bottom-center. Changing the pivot
    /// keeps sliders: evaluation shifts the mesh by `-pivot`.
    pub pivot: Vec3,
}

impl Shape {
    pub fn new(source: ShapeSource, material: MaterialId) -> Self {
        Self {
            source,
            role: Role::Solid,
            material,
            look: Shading::Faceted,
            effects: Effects::default(),
            pivot: Vec3::ZERO,
        }
    }

    /// The source mesh in object space (pivot applied, effects not applied).
    pub fn base_mesh(&self) -> Arc<PolyMesh> {
        let mesh = match &self.source {
            ShapeSource::Primitive(p) => Arc::new(p.build()),
            ShapeSource::Mesh(m) => m.clone(),
        };
        apply_pivot(mesh, self.pivot)
    }

    pub fn primitive(&self) -> Option<&Primitive> {
        match &self.source {
            ShapeSource::Primitive(p) => Some(p),
            ShapeSource::Mesh(_) => None,
        }
    }
}

/// Shift a mesh so `pivot` lands on the origin (no copy when the pivot is zero).
pub fn apply_pivot(mesh: Arc<PolyMesh>, pivot: Vec3) -> Arc<PolyMesh> {
    if pivot == Vec3::ZERO {
        return mesh;
    }
    let mut m = (*mesh).clone();
    m.transform(&glam::Mat4::from_translation(-pivot));
    Arc::new(m)
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShapeSource {
    /// Still has its sliders.
    Primitive(Primitive),
    /// Edited faces/points ("locked in").
    Mesh(Arc<PolyMesh>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Combine {
    /// A plain folder of objects.
    #[default]
    None,
    /// Solids joined, cutters cut away (live).
    Union,
    /// Keep only where solids overlap (live).
    Intersect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub combine: Combine,
    pub effects: Effects,
}

/// Live effects, always applied in this order: Mirror, Repeat, Smooth.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Effects {
    pub mirror: Option<MirrorFx>,
    pub repeat: Option<RepeatFx>,
    pub smooth: Option<SmoothFx>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.mirror.is_none() && self.repeat.is_none() && self.smooth.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub fn unit(self) -> Vec3 {
        match self {
            Axis::X => Vec3::X,
            Axis::Y => Vec3::Y,
            Axis::Z => Vec3::Z,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MirrorFx {
    pub enabled: bool,
    pub axis: Axis,
    /// Weld points that sit on the mirror plane.
    pub weld: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RepeatFx {
    Line { enabled: bool, count: u16, offset: Vec3 },
    Circle { enabled: bool, count: u16, axis: Axis, radius: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmoothFx {
    pub enabled: bool,
    pub levels: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LightKind {
    Point,
    Spot { angle_deg: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub kind: LightKind,
    /// sRGB 0..1.
    pub color: [f32; 3],
    pub brightness: f32,
    /// How far the light reaches, in scene units.
    pub range: f32,
    pub shadows: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self { kind: LightKind::Point, color: [1.0, 0.95, 0.85], brightness: 1.0, range: 16.0, shadows: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhotoCamera {
    pub fov_deg: f32,
}

impl Default for PhotoCamera {
    fn default() -> Self {
        Self { fov_deg: 50.0 }
    }
}
