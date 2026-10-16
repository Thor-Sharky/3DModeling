//! CPU picking: which object and face is under the cursor.

use glam::Vec3;
use ss_math::Ray;
use ss_mesh::FaceId;

use crate::{Document, Evaluator, ObjectId};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickHit {
    pub object: ObjectId,
    pub face: FaceId,
    /// Distance along the (normalized) ray.
    pub t: f32,
    pub point: Vec3,
    /// World-space surface normal.
    pub normal: Vec3,
}

/// Closest visible shape hit by a world-space ray. `filter` can skip objects
/// (e.g. locked ones, or the object being dragged).
pub fn pick_object(
    doc: &Document,
    eval: &mut Evaluator,
    ray: &Ray,
    filter: impl Fn(ObjectId) -> bool,
) -> Option<PickHit> {
    let ray = ray.normalized();
    let mut best: Option<PickHit> = None;
    for (id, obj) in doc.objects() {
        if obj.shape().is_none() || !doc.is_visible(id) || !filter(id) {
            continue;
        }
        let Some(mesh) = eval.shape_mesh(doc, id) else { continue };
        let world = doc.world_transform(id);
        let inv = world.inverse();
        // Rigid transforms keep distances, so `t` is comparable across objects.
        let local = Ray::new(inv.transform_point(ray.origin), inv.transform_vector(ray.dir));
        let bvh = mesh.bvh();
        if local.intersect_aabb(&bvh.bounds()).is_none() {
            continue;
        }
        if let Some(h) = bvh.raycast(&local)
            && best.is_none_or(|b| h.t < b.t)
        {
            let mut normal = world.transform_vector(h.normal);
            if normal.dot(ray.dir) > 0.0 {
                normal = -normal; // hit from inside or a back face
            }
            best = Some(PickHit { object: id, face: h.face, t: h.t, point: ray.at(h.t), normal });
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Object, ObjectKind, Shape, ShapeSource};
    use ss_mesh::Primitive;

    #[test]
    fn picks_nearest_and_respects_transform() {
        let mut doc = Document::default();
        let mat = doc.default_material().unwrap();
        let mut tx = doc.begin("Add");
        let mk =
            || Object::new("B", ObjectKind::Shape(Shape::new(ShapeSource::Primitive(Primitive::default_block()), mat)));
        let near = tx.add_object(mk(), None, None).unwrap();
        let far = tx.add_object(mk(), None, None).unwrap();
        tx.object_mut(near).unwrap().transform.translation = Vec3::new(0.0, 0.0, 5.0);
        tx.object_mut(far).unwrap().transform.translation = Vec3::new(0.0, 0.0, -5.0);
        tx.commit();
        let mut ev = Evaluator::new();
        let ray = Ray::new(Vec3::new(0.0, 0.0, 20.0), Vec3::NEG_Z);
        let hit = pick_object(&doc, &mut ev, &ray, |_| true).unwrap();
        assert_eq!(hit.object, near);
        assert_eq!(hit.face, FaceId(4)); // +Z side faces the camera
        assert!((hit.point.z - 6.0).abs() < 1e-5);
        assert!((hit.normal - Vec3::Z).length() < 1e-5);
        let hit = pick_object(&doc, &mut ev, &ray, |id| id != near).unwrap();
        assert_eq!(hit.object, far);
    }
}
