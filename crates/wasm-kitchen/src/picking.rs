//! Working out which slot the pointer is over.
//!
//! Every slot is a box of known size standing against the wall, so picking is a
//! ray cast against axis-aligned boxes on the CPU - no ID buffer, no GPU read
//! back, and the same code on WebGPU and WebGL2.

use glam::Vec3;

/// Below this, a ray is treated as parallel to a pair of box faces.
const PARALLEL_EPSILON: f32 = 1e-8;

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

/// A half-line in world space. `direction` is expected to be normalised, so the
/// distances [`Ray::hit`] returns are in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self {
            origin,
            direction: direction.normalize_or_zero(),
        }
    }

    /// The point `distance` along the ray.
    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }

    /// Distance to where the ray enters `aabb`, or 0 if it starts inside it.
    /// `None` when it misses, or the box is entirely behind the origin.
    ///
    /// The slab method: intersect the ray with each pair of parallel faces and
    /// keep the overlap of the three intervals.
    pub fn hit(&self, aabb: &Aabb) -> Option<f32> {
        let mut near = 0.0_f32;
        let mut far = f32::INFINITY;

        for axis in 0..3 {
            let origin = self.origin[axis];
            let direction = self.direction[axis];
            let (min, max) = (aabb.min[axis], aabb.max[axis]);

            if direction.abs() < PARALLEL_EPSILON {
                // Parallel to this slab: either always inside it or never.
                if origin < min || origin > max {
                    return None;
                }
                continue;
            }

            let inverse = 1.0 / direction;
            let (mut t0, mut t1) = ((min - origin) * inverse, (max - origin) * inverse);
            if t0 > t1 {
                core::mem::swap(&mut t0, &mut t1);
            }

            near = near.max(t0);
            far = far.min(t1);
            if near > far {
                return None;
            }
        }

        Some(near)
    }
}

/// Index of the box the ray enters first, if it hits any.
pub fn pick_nearest<'a>(ray: &Ray, boxes: impl IntoIterator<Item = &'a Aabb>) -> Option<usize> {
    boxes
        .into_iter()
        .enumerate()
        .filter_map(|(index, aabb)| ray.hit(aabb).map(|distance| (index, distance)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box_at(center: Vec3) -> Aabb {
        Aabb::new(center - Vec3::splat(0.5), center + Vec3::splat(0.5))
    }

    #[test]
    fn a_ray_straight_at_a_box_hits_its_near_face() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z);
        let distance = ray.hit(&unit_box_at(Vec3::ZERO)).expect("should hit");

        assert!((distance - 4.5).abs() < 1e-5);
        assert!((ray.at(distance).z - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_ray_that_passes_beside_a_box_misses() {
        let ray = Ray::new(Vec3::new(2.0, 0.0, 5.0), Vec3::NEG_Z);
        assert_eq!(ray.hit(&unit_box_at(Vec3::ZERO)), None);
    }

    #[test]
    fn a_box_behind_the_origin_is_not_hit() {
        let ray = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::Z);
        assert_eq!(ray.hit(&unit_box_at(Vec3::ZERO)), None);
    }

    #[test]
    fn a_ray_starting_inside_a_box_hits_at_zero() {
        let ray = Ray::new(Vec3::ZERO, Vec3::new(0.3, 0.2, -1.0));
        assert_eq!(ray.hit(&unit_box_at(Vec3::ZERO)), Some(0.0));
    }

    #[test]
    fn an_axis_parallel_ray_is_handled_without_dividing_by_zero() {
        // Along X only: the Y and Z slabs are parallel to it.
        let inside = Ray::new(Vec3::new(-5.0, 0.2, -0.2), Vec3::X);
        let outside = Ray::new(Vec3::new(-5.0, 0.8, 0.0), Vec3::X);

        assert!(inside.hit(&unit_box_at(Vec3::ZERO)).is_some());
        assert_eq!(outside.hit(&unit_box_at(Vec3::ZERO)), None);
    }

    #[test]
    fn a_diagonal_ray_can_graze_past_a_corner() {
        let ray = Ray::new(Vec3::new(-2.0, 0.0, 2.0), Vec3::new(1.0, 0.0, -0.2));
        assert_eq!(ray.hit(&unit_box_at(Vec3::ZERO)), None);
    }

    #[test]
    fn picking_returns_the_nearest_of_several_boxes() {
        let boxes = [
            unit_box_at(Vec3::new(0.0, 0.0, -4.0)),
            unit_box_at(Vec3::new(0.0, 0.0, -1.0)),
            unit_box_at(Vec3::new(3.0, 0.0, -1.0)),
        ];
        let ray = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z);

        assert_eq!(pick_nearest(&ray, &boxes), Some(1));
    }

    #[test]
    fn picking_nothing_returns_none() {
        let boxes = [unit_box_at(Vec3::new(3.0, 0.0, 0.0))];
        let ray = Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z);

        assert_eq!(pick_nearest(&ray, &boxes), None);
    }
}
