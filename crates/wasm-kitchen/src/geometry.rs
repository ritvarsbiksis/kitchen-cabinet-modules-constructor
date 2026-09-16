//! The meshes the room is built from that do not come from a `.glb`: the floor
//! slab, the wall and the pendant lamps.
//!
//! They are built in world space in the same vertex format the glTF loader
//! produces, so the renderer draws them through the same pipeline as the
//! modules.

use core::f32::consts::TAU;

use glam::Vec3;
use scene_assets::model::Vertex;

use crate::picking::Aabb;

/// An indexed triangle list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// Add a flat quad. `corners` go counter-clockwise seen from the side
    /// `normal` points to, and `uvs` pair up with them.
    fn push_quad(&mut self, corners: [Vec3; 4], normal: Vec3, uvs: [[f32; 2]; 4]) {
        let base = self.vertices.len() as u32;
        for (corner, uv) in corners.into_iter().zip(uvs) {
            self.vertices.push(Vertex {
                position: corner.to_array(),
                normal: normal.to_array(),
                uv,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Append another mesh's triangles to this one.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices
            .extend(other.indices.iter().map(|index| index + base));
    }

    pub fn bounds(&self) -> Aabb {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for vertex in &self.vertices {
            min = min.min(Vec3::from(vertex.position));
            max = max.max(Vec3::from(vertex.position));
        }
        Aabb::new(min, max)
    }
}

/// A box with a separate set of vertices per face, so every edge stays sharp.
///
/// Texture coordinates are the face's own world-space extents divided by
/// `uv_period`: a texture that tiles every `uv_period` metres repeats at the same
/// real size on every face, whatever the box's proportions.
pub fn cuboid(bounds: Aabb, uv_period: f32) -> Mesh {
    let Aabb { min, max } = bounds;
    let uv = |a: f32, b: f32| [a / uv_period, b / uv_period];
    let mut mesh = Mesh::default();

    // +Y, the top: U along X, V along Z - which is the floor's texture mapping.
    mesh.push_quad(
        [
            Vec3::new(min.x, max.y, max.z),
            Vec3::new(max.x, max.y, max.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
        ],
        Vec3::Y,
        [
            uv(min.x, max.z),
            uv(max.x, max.z),
            uv(max.x, min.z),
            uv(min.x, min.z),
        ],
    );
    // -Y, the bottom.
    mesh.push_quad(
        [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(min.x, min.y, max.z),
        ],
        Vec3::NEG_Y,
        [
            uv(min.x, min.z),
            uv(max.x, min.z),
            uv(max.x, max.z),
            uv(min.x, max.z),
        ],
    );
    // +Z, the front.
    mesh.push_quad(
        [
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, max.y, max.z),
            Vec3::new(min.x, max.y, max.z),
        ],
        Vec3::Z,
        [
            uv(min.x, -min.y),
            uv(max.x, -min.y),
            uv(max.x, -max.y),
            uv(min.x, -max.y),
        ],
    );
    // -Z, the back.
    mesh.push_quad(
        [
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(max.x, max.y, min.z),
        ],
        Vec3::NEG_Z,
        [
            uv(-max.x, -min.y),
            uv(-min.x, -min.y),
            uv(-min.x, -max.y),
            uv(-max.x, -max.y),
        ],
    );
    // +X, the right.
    mesh.push_quad(
        [
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(max.x, max.y, max.z),
        ],
        Vec3::X,
        [
            uv(-max.z, -min.y),
            uv(-min.z, -min.y),
            uv(-min.z, -max.y),
            uv(-max.z, -max.y),
        ],
    );
    // -X, the left.
    mesh.push_quad(
        [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(min.x, max.y, max.z),
            Vec3::new(min.x, max.y, min.z),
        ],
        Vec3::NEG_X,
        [
            uv(min.z, -min.y),
            uv(max.z, -min.y),
            uv(max.z, -max.y),
            uv(min.z, -max.y),
        ],
    );

    mesh
}

/// The side of a truncated cone around a vertical axis, open at both ends.
///
/// `top` is the centre of the upper rim; the lower rim is `height` below it.
/// Normals point outwards, tilted by the slope, so a flared lamp shade catches
/// light on its outside the way a real one does.
pub fn cone_shell(
    top: Vec3,
    top_radius: f32,
    bottom_radius: f32,
    height: f32,
    segments: u32,
) -> Mesh {
    let mut mesh = Mesh::default();
    let segments = segments.max(3);
    // Outward and up by as much as the radius grows over the height.
    let slope = bottom_radius - top_radius;

    for index in 0..=segments {
        let angle = index as f32 / segments as f32 * TAU;
        let (sin, cos) = angle.sin_cos();
        let radial = Vec3::new(cos, 0.0, sin);
        let normal = (radial * height + Vec3::Y * slope).normalize_or_zero();
        let u = index as f32 / segments as f32;

        for (radius, y, v) in [
            (top_radius, top.y, 0.0),
            (bottom_radius, top.y - height, 1.0),
        ] {
            mesh.vertices.push(Vertex {
                position: (Vec3::new(top.x, y, top.z) + radial * radius).to_array(),
                normal: normal.to_array(),
                uv: [u, v],
            });
        }
    }

    for index in 0..segments {
        let upper = index * 2;
        let lower = upper + 1;
        let next_upper = upper + 2;
        let next_lower = upper + 3;
        mesh.indices
            .extend_from_slice(&[upper, next_upper, lower, lower, next_upper, next_lower]);
    }

    mesh
}

/// A flat, horizontal disc facing down - the glowing diffuser under a shade.
pub fn disc_facing_down(center: Vec3, radius: f32, segments: u32) -> Mesh {
    let mut mesh = Mesh::default();
    let segments = segments.max(3);

    mesh.vertices.push(Vertex {
        position: center.to_array(),
        normal: Vec3::NEG_Y.to_array(),
        uv: [0.5, 0.5],
    });
    for index in 0..=segments {
        let angle = index as f32 / segments as f32 * TAU;
        let (sin, cos) = angle.sin_cos();
        mesh.vertices.push(Vertex {
            position: (center + Vec3::new(cos, 0.0, sin) * radius).to_array(),
            normal: Vec3::NEG_Y.to_array(),
            uv: [0.5 + cos * 0.5, 0.5 + sin * 0.5],
        });
    }
    for index in 1..=segments {
        mesh.indices.extend_from_slice(&[0, index, index + 1]);
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_well_formed(mesh: &Mesh) {
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(mesh
            .indices
            .iter()
            .all(|&index| (index as usize) < mesh.vertices.len()));
        for vertex in &mesh.vertices {
            let length = Vec3::from(vertex.normal).length();
            assert!((length - 1.0).abs() < 1e-4, "normal of length {length}");
        }
    }

    #[test]
    fn a_cuboid_has_six_sharp_faces_and_the_requested_bounds() {
        let bounds = Aabb::new(Vec3::new(-1.0, 0.0, -3.0), Vec3::new(2.0, 2.5, -2.8));
        let mesh = cuboid(bounds, 1.0);

        assert_well_formed(&mesh);
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
        assert_eq!(mesh.bounds(), bounds);
    }

    #[test]
    fn cuboid_faces_wind_counter_clockwise_around_their_normals() {
        let mesh = cuboid(Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0)), 1.0);

        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from(mesh.vertices[triangle[i] as usize].position));
            let face = (b - a).cross(c - a).normalize();
            let normal = Vec3::from(mesh.vertices[triangle[0] as usize].normal);
            assert!(face.dot(normal) > 0.99);
        }
    }

    #[test]
    fn the_top_face_tiles_at_the_uv_period() {
        let mesh = cuboid(
            Aabb::new(Vec3::new(-3.0, -0.1, -3.0), Vec3::new(3.0, 0.0, 3.0)),
            2.4,
        );
        let top = &mesh.vertices[..4];

        let u_span = top
            .iter()
            .map(|v| v.uv[0])
            .fold(f32::NEG_INFINITY, f32::max)
            - top.iter().map(|v| v.uv[0]).fold(f32::INFINITY, f32::min);
        assert!((u_span - 6.0 / 2.4).abs() < 1e-4);
    }

    #[test]
    fn a_flared_shade_has_outward_normals_and_its_rims_where_asked() {
        let top = Vec3::new(0.5, 2.0, 0.0);
        let mesh = cone_shell(top, 0.05, 0.2, 0.25, 24);

        assert_well_formed(&mesh);
        let bounds = mesh.bounds();
        assert!((bounds.max.y - 2.0).abs() < 1e-5);
        assert!((bounds.min.y - 1.75).abs() < 1e-5);
        assert!((bounds.size().x - 0.4).abs() < 1e-3);

        for vertex in &mesh.vertices {
            let outward = Vec3::from(vertex.position) - Vec3::new(top.x, vertex.position[1], top.z);
            assert!(Vec3::from(vertex.normal).dot(outward) > 0.0);
            // Flaring out, so the outside faces a little upwards.
            assert!(vertex.normal[1] > 0.0);
        }
    }

    #[test]
    fn the_diffuser_disc_faces_down() {
        let mesh = disc_facing_down(Vec3::new(0.0, 1.8, 0.0), 0.18, 16);

        assert_well_formed(&mesh);
        assert!(mesh
            .vertices
            .iter()
            .all(|vertex| vertex.normal == [0.0, -1.0, 0.0]));
        for triangle in mesh.indices.chunks_exact(3) {
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from(mesh.vertices[triangle[i] as usize].position));
            assert!((b - a).cross(c - a).y < 0.0, "should wind towards -Y");
        }
    }

    #[test]
    fn appending_offsets_the_indices() {
        let mut mesh = cuboid(Aabb::new(Vec3::ZERO, Vec3::ONE), 1.0);
        mesh.append(&cuboid(Aabb::new(Vec3::splat(2.0), Vec3::splat(3.0)), 1.0));

        assert_well_formed(&mesh);
        assert_eq!(mesh.vertices.len(), 48);
        assert_eq!(mesh.bounds(), Aabb::new(Vec3::ZERO, Vec3::splat(3.0)));
    }
}
