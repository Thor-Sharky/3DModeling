//! The starter shapes. Every shape is centered on its bounding box at the origin,
//! closed, and wound counter-clockwise seen from outside.
//!
//! Face ids are fixed per shape so quests, scripts and tests can name faces:
//! - Block: 0 +X, 1 -X, 2 +Y (top), 3 -Y (bottom), 4 +Z (back), 5 -Z (front).
//! - Cylinder, Cone, Pyramid: 0 bottom, then (Cylinder) 1 top, then the sides.
//! - Wedge: 0 bottom, 1 back (+Z), 2 slope, 3 right (+X), 4 left (-X).
//!
//! Remember: the model's front faces -Z.

use std::f32::consts::TAU;

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::{PolyMesh, VertId};

/// A shape that still has its sliders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Primitive {
    Block { size: Vec3, roundness: f32, segs: u8 },
    Ball { size: Vec3, detail: u8 },
    Cylinder { size: Vec3, sides: u16, roundness: f32 },
    Wedge { size: Vec3 },
    CornerWedge { size: Vec3 },
    Cone { size: Vec3, sides: u16 },
    Pyramid { size: Vec3, sides: u16 },
    Ring { size: Vec3, thickness: f32, sides: u16, ring_sides: u16 },
    Tube { size: Vec3, thickness: f32, sides: u16 },
}

/// The kinds in the Shapes panel, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveKind {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
    Cone,
    Pyramid,
    Ring,
    Tube,
}

impl PrimitiveKind {
    pub const ALL: [PrimitiveKind; 9] = [
        PrimitiveKind::Block,
        PrimitiveKind::Ball,
        PrimitiveKind::Cylinder,
        PrimitiveKind::Wedge,
        PrimitiveKind::CornerWedge,
        PrimitiveKind::Cone,
        PrimitiveKind::Pyramid,
        PrimitiveKind::Ring,
        PrimitiveKind::Tube,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PrimitiveKind::Block => "Block",
            PrimitiveKind::Ball => "Ball",
            PrimitiveKind::Cylinder => "Cylinder",
            PrimitiveKind::Wedge => "Wedge",
            PrimitiveKind::CornerWedge => "Corner Wedge",
            PrimitiveKind::Cone => "Cone",
            PrimitiveKind::Pyramid => "Pyramid",
            PrimitiveKind::Ring => "Ring",
            PrimitiveKind::Tube => "Tube",
        }
    }

    /// Stable id used in tool ids and files, e.g. `shape.add.corner_wedge`.
    pub fn key(self) -> &'static str {
        match self {
            PrimitiveKind::Block => "block",
            PrimitiveKind::Ball => "ball",
            PrimitiveKind::Cylinder => "cylinder",
            PrimitiveKind::Wedge => "wedge",
            PrimitiveKind::CornerWedge => "corner_wedge",
            PrimitiveKind::Cone => "cone",
            PrimitiveKind::Pyramid => "pyramid",
            PrimitiveKind::Ring => "ring",
            PrimitiveKind::Tube => "tube",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }

    /// The shape you get when you click its button.
    pub fn default_primitive(self) -> Primitive {
        match self {
            PrimitiveKind::Block => Primitive::default_block(),
            PrimitiveKind::Ball => Primitive::Ball { size: Vec3::splat(2.0), detail: 2 },
            PrimitiveKind::Cylinder => {
                Primitive::Cylinder { size: Vec3::new(2.0, 2.0, 2.0), sides: 16, roundness: 0.0 }
            }
            PrimitiveKind::Wedge => Primitive::Wedge { size: Vec3::new(4.0, 1.0, 2.0) },
            PrimitiveKind::CornerWedge => Primitive::CornerWedge { size: Vec3::new(2.0, 2.0, 2.0) },
            PrimitiveKind::Cone => Primitive::Cone { size: Vec3::new(2.0, 2.0, 2.0), sides: 16 },
            PrimitiveKind::Pyramid => Primitive::Pyramid { size: Vec3::new(2.0, 2.0, 2.0), sides: 4 },
            PrimitiveKind::Ring => {
                Primitive::Ring { size: Vec3::new(4.0, 1.0, 4.0), thickness: 1.0, sides: 24, ring_sides: 12 }
            }
            PrimitiveKind::Tube => Primitive::Tube { size: Vec3::new(2.0, 4.0, 2.0), thickness: 0.4, sides: 16 },
        }
    }
}

pub const MIN_SIZE: f32 = 0.05;

impl Primitive {
    /// A Roblox-Part-sized block: 4 x 1 x 2 studs.
    pub fn default_block() -> Self {
        Primitive::Block { size: Vec3::new(4.0, 1.0, 2.0), roundness: 0.0, segs: 2 }
    }

    pub fn kind(&self) -> PrimitiveKind {
        match self {
            Primitive::Block { .. } => PrimitiveKind::Block,
            Primitive::Ball { .. } => PrimitiveKind::Ball,
            Primitive::Cylinder { .. } => PrimitiveKind::Cylinder,
            Primitive::Wedge { .. } => PrimitiveKind::Wedge,
            Primitive::CornerWedge { .. } => PrimitiveKind::CornerWedge,
            Primitive::Cone { .. } => PrimitiveKind::Cone,
            Primitive::Pyramid { .. } => PrimitiveKind::Pyramid,
            Primitive::Ring { .. } => PrimitiveKind::Ring,
            Primitive::Tube { .. } => PrimitiveKind::Tube,
        }
    }

    pub fn size(&self) -> Vec3 {
        match self {
            Primitive::Block { size, .. }
            | Primitive::Ball { size, .. }
            | Primitive::Cylinder { size, .. }
            | Primitive::Wedge { size }
            | Primitive::CornerWedge { size }
            | Primitive::Cone { size, .. }
            | Primitive::Pyramid { size, .. }
            | Primitive::Ring { size, .. }
            | Primitive::Tube { size, .. } => *size,
        }
    }

    /// Change the overall size (each axis clamped to at least [`MIN_SIZE`]).
    pub fn set_size(&mut self, new: Vec3) {
        let new = new.max(Vec3::splat(MIN_SIZE));
        match self {
            Primitive::Block { size, .. }
            | Primitive::Ball { size, .. }
            | Primitive::Cylinder { size, .. }
            | Primitive::Wedge { size }
            | Primitive::CornerWedge { size }
            | Primitive::Cone { size, .. }
            | Primitive::Pyramid { size, .. }
            | Primitive::Ring { size, .. }
            | Primitive::Tube { size, .. } => *size = new,
        }
    }

    /// Build the mesh. Out-of-range settings are clamped, never rejected.
    pub fn build(&self) -> PolyMesh {
        match self {
            Primitive::Block { size, .. } => block(*size),
            Primitive::Ball { size, detail } => ball(*size, *detail),
            Primitive::Cylinder { size, sides, .. } => cylinder(*size, *sides),
            Primitive::Wedge { size } => wedge(*size),
            Primitive::CornerWedge { size } => corner_wedge(*size),
            Primitive::Cone { size, sides } => cone(*size, (*sides).max(3)),
            Primitive::Pyramid { size, sides } => cone(*size, (*sides).clamp(3, 64)),
            Primitive::Ring { size, thickness, sides, ring_sides } => ring(*size, *thickness, *sides, *ring_sides),
            Primitive::Tube { size, thickness, sides } => tube(*size, *thickness, *sides),
        }
    }
}

fn clamp_size(size: Vec3) -> Vec3 {
    size.max(Vec3::splat(MIN_SIZE))
}

fn add_verts(m: &mut PolyMesh, pts: impl IntoIterator<Item = Vec3>) -> Vec<VertId> {
    pts.into_iter().map(|p| m.add_vert(p)).collect()
}

fn face(m: &mut PolyMesh, ids: &[VertId], idx: &[usize]) {
    let vs: Vec<VertId> = idx.iter().map(|i| ids[*i]).collect();
    m.add_face(&vs, None).expect("primitive faces are valid");
}

fn block(size: Vec3) -> PolyMesh {
    let h = clamp_size(size) * 0.5;
    let mut m = PolyMesh::new();
    // Corner i: x = bit0, y = bit1, z = bit2.
    let v = add_verts(
        &mut m,
        (0..8).map(|i| {
            Vec3::new(
                if i & 1 != 0 { h.x } else { -h.x },
                if i & 2 != 0 { h.y } else { -h.y },
                if i & 4 != 0 { h.z } else { -h.z },
            )
        }),
    );
    face(&mut m, &v, &[1, 3, 7, 5]); // 0: +X
    face(&mut m, &v, &[0, 4, 6, 2]); // 1: -X
    face(&mut m, &v, &[2, 6, 7, 3]); // 2: +Y top
    face(&mut m, &v, &[0, 1, 5, 4]); // 3: -Y bottom
    face(&mut m, &v, &[4, 5, 7, 6]); // 4: +Z back
    face(&mut m, &v, &[0, 2, 3, 1]); // 5: -Z front
    m
}

fn circle(sides: u16, radius: Vec2) -> Vec<Vec2> {
    (0..sides)
        .map(|i| {
            let a = i as f32 / sides as f32 * TAU;
            Vec2::new(a.cos() * radius.x, -a.sin() * radius.y)
        })
        .collect()
}

fn cylinder(size: Vec3, sides: u16) -> PolyMesh {
    let s = clamp_size(size);
    let sides = sides.clamp(3, 256);
    let ring = circle(sides, Vec2::new(s.x, s.z) * 0.5);
    let mut m = PolyMesh::new();
    let bottom = add_verts(&mut m, ring.iter().map(|p| Vec3::new(p.x, -s.y * 0.5, p.y)));
    let top = add_verts(&mut m, ring.iter().map(|p| Vec3::new(p.x, s.y * 0.5, p.y)));
    let n = sides as usize;
    // Ring points go counter-clockwise seen from above (x = cos, z = -sin).
    let bottom_face: Vec<VertId> = bottom.iter().rev().copied().collect();
    m.add_face(&bottom_face, None).unwrap(); // 0: bottom (seen from below)
    m.add_face(&top, None).unwrap(); // 1: top
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_face(&[bottom[i], bottom[j], top[j], top[i]], None).unwrap();
    }
    m
}

fn cone(size: Vec3, sides: u16) -> PolyMesh {
    let s = clamp_size(size);
    let ring = circle(sides.min(256), Vec2::new(s.x, s.z) * 0.5);
    let mut m = PolyMesh::new();
    let base = add_verts(&mut m, ring.iter().map(|p| Vec3::new(p.x, -s.y * 0.5, p.y)));
    let apex = m.add_vert(Vec3::new(0.0, s.y * 0.5, 0.0));
    let rev: Vec<VertId> = base.iter().rev().copied().collect();
    m.add_face(&rev, None).unwrap(); // 0: bottom
    let n = base.len();
    for i in 0..n {
        m.add_face(&[base[i], base[(i + 1) % n], apex], None).unwrap();
    }
    m
}

fn wedge(size: Vec3) -> PolyMesh {
    // Like a Roblox WedgePart: tall at the back (+Z), sloping down to the front (-Z).
    let h = clamp_size(size) * 0.5;
    let mut m = PolyMesh::new();
    let v = add_verts(
        &mut m,
        [
            Vec3::new(-h.x, -h.y, -h.z), // 0 front-left-bottom
            Vec3::new(h.x, -h.y, -h.z),  // 1 front-right-bottom
            Vec3::new(-h.x, -h.y, h.z),  // 2 back-left-bottom
            Vec3::new(h.x, -h.y, h.z),   // 3 back-right-bottom
            Vec3::new(-h.x, h.y, h.z),   // 4 back-left-top
            Vec3::new(h.x, h.y, h.z),    // 5 back-right-top
        ],
    );
    face(&mut m, &v, &[0, 1, 3, 2]); // 0: bottom
    face(&mut m, &v, &[2, 3, 5, 4]); // 1: back
    face(&mut m, &v, &[0, 4, 5, 1]); // 2: slope
    face(&mut m, &v, &[1, 5, 3]); // 3: right
    face(&mut m, &v, &[0, 2, 4]); // 4: left
    m
}

fn corner_wedge(size: Vec3) -> PolyMesh {
    // Square base with the peak above the back-right corner.
    let h = clamp_size(size) * 0.5;
    let mut m = PolyMesh::new();
    let v = add_verts(
        &mut m,
        [
            Vec3::new(-h.x, -h.y, -h.z), // 0 front-left
            Vec3::new(h.x, -h.y, -h.z),  // 1 front-right
            Vec3::new(-h.x, -h.y, h.z),  // 2 back-left
            Vec3::new(h.x, -h.y, h.z),   // 3 back-right
            Vec3::new(h.x, h.y, h.z),    // 4 peak
        ],
    );
    face(&mut m, &v, &[0, 1, 3, 2]); // 0: bottom
    face(&mut m, &v, &[1, 4, 3]); // 1: right (vertical)
    face(&mut m, &v, &[2, 3, 4]); // 2: back (vertical)
    face(&mut m, &v, &[0, 4, 1]); // 3: front slope
    face(&mut m, &v, &[0, 2, 4]); // 4: left slope
    m
}

fn ball(size: Vec3, detail: u8) -> PolyMesh {
    let r = clamp_size(size) * 0.5;
    let segs = 8 + 4 * detail.min(6) as usize; // around
    let rings = segs / 2; // pole to pole
    let mut m = PolyMesh::new();
    let top = m.add_vert(Vec3::new(0.0, r.y, 0.0));
    let mut rows = Vec::with_capacity(rings - 1);
    for j in 1..rings {
        let phi = j as f32 / rings as f32 * std::f32::consts::PI;
        let (sp, cp) = phi.sin_cos();
        let row = add_verts(
            &mut m,
            (0..segs).map(|i| {
                let a = i as f32 / segs as f32 * TAU;
                Vec3::new(a.cos() * sp * r.x, cp * r.y, -a.sin() * sp * r.z)
            }),
        );
        rows.push(row);
    }
    let bottom = m.add_vert(Vec3::new(0.0, -r.y, 0.0));
    for i in 0..segs {
        let j = (i + 1) % segs;
        m.add_face(&[top, rows[0][i], rows[0][j]], None).unwrap();
    }
    for k in 0..rows.len() - 1 {
        for i in 0..segs {
            let j = (i + 1) % segs;
            m.add_face(&[rows[k][j], rows[k][i], rows[k + 1][i], rows[k + 1][j]], None).unwrap();
        }
    }
    let last = rows.last().unwrap();
    for i in 0..segs {
        let j = (i + 1) % segs;
        m.add_face(&[bottom, last[j], last[i]], None).unwrap();
    }
    m
}

fn ring(size: Vec3, thickness: f32, sides: u16, ring_sides: u16) -> PolyMesh {
    let s = clamp_size(size);
    let half_xz = Vec2::new(s.x, s.z) * 0.5;
    let tube_r = (thickness * 0.5).clamp(MIN_SIZE * 0.5, half_xz.min_element() * 0.95);
    let sy = s.y * 0.5 / tube_r; // vertical squash so the ring is exactly size.y tall
    let sides = sides.clamp(3, 256) as usize;
    let ring_sides = ring_sides.clamp(3, 64) as usize;
    let mut m = PolyMesh::new();
    let mut grid = Vec::with_capacity(sides);
    for i in 0..sides {
        let a = i as f32 / sides as f32 * TAU;
        let dir = Vec3::new(a.cos(), 0.0, -a.sin());
        let center = Vec3::new(dir.x * (half_xz.x - tube_r), 0.0, dir.z * (half_xz.y - tube_r));
        let row = add_verts(
            &mut m,
            (0..ring_sides).map(|k| {
                let b = k as f32 / ring_sides as f32 * TAU;
                center + dir * (b.cos() * tube_r) + Vec3::Y * (b.sin() * tube_r * sy)
            }),
        );
        grid.push(row);
    }
    for i in 0..sides {
        let i2 = (i + 1) % sides;
        for k in 0..ring_sides {
            let k2 = (k + 1) % ring_sides;
            m.add_face(&[grid[i][k], grid[i2][k], grid[i2][k2], grid[i][k2]], None).unwrap();
        }
    }
    m
}

fn tube(size: Vec3, thickness: f32, sides: u16) -> PolyMesh {
    let s = clamp_size(size);
    let outer_r = Vec2::new(s.x, s.z) * 0.5;
    let t = thickness.clamp(MIN_SIZE * 0.5, outer_r.min_element() * 0.95);
    let inner_r = outer_r - Vec2::splat(t);
    let sides = sides.clamp(3, 256);
    let outer = circle(sides, outer_r);
    let inner = circle(sides, inner_r);
    let (yb, yt) = (-s.y * 0.5, s.y * 0.5);
    let mut m = PolyMesh::new();
    let ob = add_verts(&mut m, outer.iter().map(|p| Vec3::new(p.x, yb, p.y)));
    let ot = add_verts(&mut m, outer.iter().map(|p| Vec3::new(p.x, yt, p.y)));
    let ib = add_verts(&mut m, inner.iter().map(|p| Vec3::new(p.x, yb, p.y)));
    let it = add_verts(&mut m, inner.iter().map(|p| Vec3::new(p.x, yt, p.y)));
    let n = sides as usize;
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_face(&[ot[i], ot[j], it[j], it[i]], None).unwrap(); // top
        m.add_face(&[ob[j], ob[i], ib[i], ib[j]], None).unwrap(); // bottom
        m.add_face(&[ob[i], ob[j], ot[j], ot[i]], None).unwrap(); // outside
        m.add_face(&[ib[j], ib[i], it[i], it[j]], None).unwrap(); // inside
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FaceId;

    #[test]
    fn every_default_shape_is_solid_with_expected_euler() {
        for kind in PrimitiveKind::ALL {
            let m = kind.default_primitive().build();
            let h = m.health();
            assert!(h.is_solid(), "{kind:?}: {h:?}");
            let expected = if matches!(kind, PrimitiveKind::Ring | PrimitiveKind::Tube) { 0 } else { 2 };
            assert_eq!(h.euler_characteristic(), expected, "{kind:?}");
            let b = m.bounds();
            assert!((b.center()).length() < 1e-4, "{kind:?} is not centered: {:?}", b.center());
            assert!((b.size() - kind.default_primitive().size()).length() < 1e-3, "{kind:?} size {:?}", b.size());
        }
    }

    #[test]
    fn block_faces_have_fixed_directions() {
        let m = Primitive::default_block().build();
        let expect = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z];
        for (i, n) in expect.iter().enumerate() {
            let got = m.face_normal(FaceId(i as u32)).unwrap();
            assert!((got - *n).length() < 1e-6, "face {i}: {got}");
        }
        assert!((m.signed_volume() - 8.0).abs() < 1e-6);
    }

    #[test]
    fn wedge_faces_have_fixed_directions() {
        let m = PrimitiveKind::Wedge.default_primitive().build();
        assert!((m.face_normal(FaceId(0)).unwrap() - Vec3::NEG_Y).length() < 1e-6);
        assert!((m.face_normal(FaceId(1)).unwrap() - Vec3::Z).length() < 1e-6);
        let slope = m.face_normal(FaceId(2)).unwrap();
        assert!(slope.y > 0.0 && slope.z < 0.0, "{slope}");
    }

    #[test]
    fn keys_roundtrip() {
        for k in PrimitiveKind::ALL {
            assert_eq!(PrimitiveKind::from_key(k.key()), Some(k));
        }
    }
}
