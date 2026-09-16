//! Loading a binary glTF (`.glb`) into flat, GPU-ready buffers.
//!
//! Nothing in here touches the GPU or the DOM, so it builds for the host target
//! and is covered by the unit tests at the bottom of the file.
//!
//! Node transforms are baked into the vertices at load time, and the whole model
//! is then recentred on the origin and scaled to a unit size. That means the
//! renderer never needs a per-draw model matrix and the camera can work in the
//! same units for any asset, however the exporter happened to scale it.

use glam::{Mat3, Mat4, Vec3};

/// A single vertex as the shader consumes it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// The subset of glTF's PBR metallic-roughness material the renderer implements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Index into [`Model::textures`], when the material has a base colour map.
    pub base_color_texture: Option<usize>,
    /// `KHR_materials_transmission`; drives how see-through the surface is drawn.
    pub transmission: f32,
}

impl Material {
    /// Whether this material has to go through the blended pass.
    pub fn is_transparent(&self) -> bool {
        self.transmission > 0.0 || self.base_color[3] < 1.0
    }

    /// Opacity to render with: transmissive glass keeps a little of its own
    /// colour rather than disappearing completely.
    pub fn opacity(&self) -> f32 {
        if self.transmission > 0.0 {
            (1.0 - self.transmission * 0.78).clamp(0.05, 1.0)
        } else {
            self.base_color[3]
        }
    }
}

/// One draw call: an indexed triangle list sharing a single material.
#[derive(Clone, Debug)]
pub struct Primitive {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: Material,
    /// Average vertex position, used to depth-sort the transparent primitives.
    pub centroid: [f32; 3],
}

/// A decoded RGBA8 image.
#[derive(Clone, Debug)]
pub struct TextureData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Everything the renderer needs from a `.glb` file.
#[derive(Clone, Debug)]
pub struct Model {
    pub primitives: Vec<Primitive>,
    pub textures: Vec<TextureData>,
    /// Radius of the bounding sphere after normalisation, so the camera can
    /// frame the model without knowing anything else about it.
    pub radius: f32,
}

/// Why a `.glb` could not be turned into a [`Model`].
#[derive(Debug)]
pub enum ModelError {
    Gltf(gltf::Error),
    /// A GLB without its binary chunk, or an asset referencing external files.
    MissingBuffer,
    UnsupportedImage(String),
    Image(image::ImageError),
    NoGeometry,
}

impl core::fmt::Display for ModelError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Gltf(error) => write!(f, "could not parse the glTF document: {error}"),
            Self::MissingBuffer => write!(
                f,
                "the asset references buffer data that is not embedded in the .glb"
            ),
            Self::UnsupportedImage(mime) => write!(f, "unsupported texture encoding `{mime}`"),
            Self::Image(error) => write!(f, "could not decode a texture: {error}"),
            Self::NoGeometry => write!(f, "the asset contains no triangle geometry"),
        }
    }
}

impl From<gltf::Error> for ModelError {
    fn from(error: gltf::Error) -> Self {
        Self::Gltf(error)
    }
}

impl From<image::ImageError> for ModelError {
    fn from(error: image::ImageError) -> Self {
        Self::Image(error)
    }
}

impl Model {
    /// Parse a binary glTF file.
    ///
    /// Only self-contained `.glb` files are supported: buffers and images have to
    /// live in the binary chunk, which is how the viewer's asset is authored and
    /// what keeps loading to a single network request.
    pub fn from_glb(bytes: &[u8]) -> Result<Self, ModelError> {
        let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(bytes)?;
        let blob = blob.ok_or(ModelError::MissingBuffer)?;

        let textures = decode_images(&document, &blob)?;

        let mut primitives = Vec::new();
        for scene in document.scenes() {
            for node in scene.nodes() {
                visit_node(&node, Mat4::IDENTITY, &blob, &mut primitives)?;
            }
        }

        if primitives
            .iter()
            .all(|primitive| primitive.indices.is_empty())
        {
            return Err(ModelError::NoGeometry);
        }

        let mut model = Self {
            primitives,
            textures,
            radius: 1.0,
        };
        model.normalise();
        Ok(model)
    }

    /// Total triangle count, for the stats the UI shows.
    pub fn triangle_count(&self) -> usize {
        self.primitives
            .iter()
            .map(|primitive| primitive.indices.len() / 3)
            .sum()
    }

    /// Recentre on the origin and scale so the bounding sphere has radius 1.
    fn normalise(&mut self) {
        let (min, max) = self.bounds();
        let center = (min + max) * 0.5;
        let extent = (max - center).length().max(f32::EPSILON);
        let scale = 1.0 / extent;

        for primitive in &mut self.primitives {
            let mut sum = Vec3::ZERO;
            for vertex in &mut primitive.vertices {
                let position = (Vec3::from(vertex.position) - center) * scale;
                vertex.position = position.to_array();
                sum += position;
            }
            let count = primitive.vertices.len().max(1) as f32;
            primitive.centroid = (sum / count).to_array();
        }

        self.radius = 1.0;
    }

    /// Axis-aligned bounds across every primitive.
    fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);

        for primitive in &self.primitives {
            for vertex in &primitive.vertices {
                let position = Vec3::from(vertex.position);
                min = min.min(position);
                max = max.max(position);
            }
        }

        if min.is_finite() && max.is_finite() {
            (min, max)
        } else {
            (Vec3::ZERO, Vec3::ZERO)
        }
    }
}

/// Walk the node hierarchy, accumulating transforms and flattening every mesh
/// primitive it finds into world space.
fn visit_node(
    node: &gltf::Node<'_>,
    parent: Mat4,
    blob: &[u8],
    out: &mut Vec<Primitive>,
) -> Result<(), ModelError> {
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            out.push(load_primitive(&mesh, &primitive, transform, blob)?);
        }
    }

    for child in node.children() {
        visit_node(&child, transform, blob, out)?;
    }

    Ok(())
}

/// Read one primitive's attributes and bake `transform` into them.
fn load_primitive(
    mesh: &gltf::Mesh<'_>,
    primitive: &gltf::Primitive<'_>,
    transform: Mat4,
    blob: &[u8],
) -> Result<Primitive, ModelError> {
    // Every buffer resolves to the GLB's binary chunk; anything else would be an
    // external file, which `Model::from_glb` does not support.
    let reader = primitive.reader(|buffer| match buffer.source() {
        gltf::buffer::Source::Bin => Some(blob),
        gltf::buffer::Source::Uri(_) => None,
    });

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or(ModelError::MissingBuffer)?
        .collect();

    // Normals are optional in glTF; without them the surface would be unlit, so
    // fall back to the geometric normal of the triangle each vertex belongs to.
    let normals: Vec<[f32; 3]> = match reader.read_normals() {
        Some(normals) => normals.collect(),
        None => vec![[0.0, 0.0, 0.0]; positions.len()],
    };

    let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
        Some(uvs) => uvs.into_f32().collect(),
        None => vec![[0.0, 0.0]; positions.len()],
    };

    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };

    // Normals are direction vectors, so they need the inverse transpose rather
    // than the transform itself - otherwise any non-uniform scale skews them.
    let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();

    let mut vertices = Vec::with_capacity(positions.len());
    for index in 0..positions.len() {
        let position = transform.transform_point3(Vec3::from(positions[index]));
        let normal = normal_matrix * Vec3::from(normals[index]);
        vertices.push(Vertex {
            position: position.to_array(),
            normal: normal.normalize_or_zero().to_array(),
            uv: uvs.get(index).copied().unwrap_or([0.0, 0.0]),
        });
    }

    fill_missing_normals(&mut vertices, &indices);

    Ok(Primitive {
        name: mesh.name().unwrap_or("primitive").to_owned(),
        vertices,
        indices,
        material: read_material(&primitive.material()),
        centroid: [0.0; 3],
    })
}

/// Give any zero-length normal the face normal of a triangle it is part of.
fn fill_missing_normals(vertices: &mut [Vertex], indices: &[u32]) {
    if !vertices
        .iter()
        .any(|vertex| Vec3::from(vertex.normal) == Vec3::ZERO)
    {
        return;
    }

    for &[first, second, third] in indices.as_chunks::<3>().0 {
        let [a, b, c] = [first as usize, second as usize, third as usize];
        let Some(position_a) = vertices.get(a).map(|vertex| Vec3::from(vertex.position)) else {
            continue;
        };
        let (Some(position_b), Some(position_c)) = (
            vertices.get(b).map(|vertex| Vec3::from(vertex.position)),
            vertices.get(c).map(|vertex| Vec3::from(vertex.position)),
        ) else {
            continue;
        };

        let face = (position_b - position_a)
            .cross(position_c - position_a)
            .normalize_or_zero();

        for index in [a, b, c] {
            let vertex = &mut vertices[index];
            if Vec3::from(vertex.normal) == Vec3::ZERO {
                vertex.normal = face.to_array();
            }
        }
    }
}

fn read_material(material: &gltf::Material<'_>) -> Material {
    let pbr = material.pbr_metallic_roughness();

    Material {
        base_color: pbr.base_color_factor(),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        base_color_texture: pbr
            .base_color_texture()
            .map(|info| info.texture().source().index()),
        transmission: material
            .transmission()
            .map_or(0.0, |transmission| transmission.transmission_factor()),
    }
}

/// Decode every embedded image into RGBA8, keeping glTF's image indices.
fn decode_images(document: &gltf::Document, blob: &[u8]) -> Result<Vec<TextureData>, ModelError> {
    let mut textures = Vec::new();

    for image in document.images() {
        let (view, mime_type) = match image.source() {
            gltf::image::Source::View { view, mime_type } => (view, mime_type),
            gltf::image::Source::Uri { .. } => return Err(ModelError::MissingBuffer),
        };

        if mime_type != "image/png" {
            return Err(ModelError::UnsupportedImage(mime_type.to_owned()));
        }

        let start = view.offset();
        let end = start + view.length();
        let encoded = blob.get(start..end).ok_or(ModelError::MissingBuffer)?;

        let decoded =
            image::load_from_memory_with_format(encoded, image::ImageFormat::Png)?.to_rgba8();

        textures.push(TextureData {
            width: decoded.width(),
            height: decoded.height(),
            rgba: decoded.into_raw(),
        });
    }

    Ok(textures)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The asset the web app serves, so the parser is exercised against the real
    /// file rather than a synthetic one.
    const SUNGLASSES: &[u8] =
        include_bytes!("../../../apps/web/public/models/SunglassesKhronos.glb");

    fn sunglasses() -> Model {
        Model::from_glb(SUNGLASSES).expect("the bundled .glb should parse")
    }

    #[test]
    fn loads_every_mesh_primitive() {
        let model = sunglasses();

        assert_eq!(model.primitives.len(), 8);
        assert!(model.triangle_count() > 0);
        assert!(model
            .primitives
            .iter()
            .all(|primitive| primitive.indices.len() % 3 == 0));
    }

    #[test]
    fn normalises_the_model_into_a_unit_sphere() {
        let model = sunglasses();
        let (min, max) = model.bounds();
        let center = (min + max) * 0.5;

        assert!(
            center.length() < 1e-3,
            "not centred on the origin: {center}"
        );
        assert!((max - center).length() <= 1.0 + 1e-3);
    }

    #[test]
    fn every_vertex_has_a_usable_normal() {
        let model = sunglasses();

        for primitive in &model.primitives {
            for vertex in &primitive.vertices {
                let length = Vec3::from(vertex.normal).length();
                assert!(
                    (length - 1.0).abs() < 1e-3,
                    "`{}` has a normal of length {length}",
                    primitive.name,
                );
            }
        }
    }

    #[test]
    fn decodes_the_embedded_texture_and_keeps_the_material_reference() {
        let model = sunglasses();

        assert_eq!(model.textures.len(), 1);
        let texture = &model.textures[0];
        assert_eq!(
            texture.rgba.len(),
            (texture.width * texture.height * 4) as usize
        );

        assert!(model
            .primitives
            .iter()
            .any(|primitive| primitive.material.base_color_texture == Some(0)));
    }

    #[test]
    fn reads_transmission_for_the_lenses() {
        let model = sunglasses();

        let transparent: Vec<_> = model
            .primitives
            .iter()
            .filter(|primitive| primitive.material.is_transparent())
            .collect();

        assert!(
            !transparent.is_empty(),
            "the lenses use KHR_materials_transmission and should be transparent",
        );
        for primitive in transparent {
            let opacity = primitive.material.opacity();
            assert!((0.05..1.0).contains(&opacity), "odd opacity {opacity}");
        }
    }

    #[test]
    fn rejects_bytes_that_are_not_a_glb() {
        assert!(Model::from_glb(b"definitely not a glb").is_err());
    }
}
