//! The wgpu side of the constructor: device setup, the room, the models placed
//! in it and the draw loop.
//!
//! Only compiled for `wasm32`. Device, surface, MSAA and gamma handling follow
//! `crates/wasm-viewer/src/renderer.rs`, including staying inside the WebGL2
//! downlevel limits so the same code runs on WebGPU and WebGL2. What differs is
//! the scene: a model is uploaded once and drawn at any number of placements,
//! each with its own small uniform for where it stands and how lit up it is.

use std::borrow::Cow;
use std::collections::HashMap;

use glam::{Mat4, Vec3};
use scene_assets::environment::{Environment, EnvironmentMap};
use scene_assets::model::{Model, TextureData, Vertex};
use wgpu::util::DeviceExt;

use crate::camera::RoomCamera;
use crate::floor;
use crate::geometry::{self, Mesh};
use crate::layout::KitchenLayout;
use crate::picking::Aabb;

const PREFERRED_SAMPLE_COUNT: u32 = 4;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
/// What is around the 6 x 6 m floor, as sRGB: a soft studio grey, so the room
/// reads as a model on a table rather than floating in a void.
const BACKGROUND: [f64; 3] = [0.83, 0.845, 0.86];
const FLOOR_TEXTURE_SIZE: u32 = 1024;

/// The wall: white with a hint of beige, as sRGB.
const WALL_COLOR: [f32; 3] = [0.95, 0.93, 0.88];
/// Lamp shades, cords and ceiling roses: matte black metal, as sRGB.
const LAMP_BODY_COLOR: [f32; 3] = [0.06, 0.06, 0.065];
/// What the diffuser under each shade gives off, in linear light.
const DIFFUSER_EMISSION: [f32; 3] = [5.0, 4.3, 3.4];
/// The lamps' light: a warm 2700 K-ish white, and its intensity.
const LAMP_LIGHT_COLOR: [f32; 3] = [1.0, 0.86, 0.7];
const LAMP_INTENSITY: f32 = 9.0;
/// How far a lamp's light reaches before it has faded out completely.
const LAMP_RANGE_M: f32 = 9.0;

/// Shade and fitting dimensions, in metres.
const SHADE_HEIGHT: f32 = 0.2;
const SHADE_TOP_RADIUS: f32 = 0.04;
const SHADE_BOTTOM_RADIUS: f32 = 0.14;
const CORD_HALF_WIDTH: f32 = 0.004;
const ROSE_HALF_SIZE: f32 = 0.05;
const ROSE_HEIGHT: f32 = 0.025;
const LAMP_SEGMENTS: u32 = 40;

/// Values for `MaterialUniform::emissive[3]`; they mirror `SURFACE_*` in
/// `kitchen.wgsl`.
const SURFACE_OTHER: f32 = 0.0;
const SURFACE_FLOOR: f32 = 1.0;
const SURFACE_WALL: f32 = 2.0;

/// Uniforms shared by every draw in a frame; mirrors `Globals` in `kitchen.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlobalsUniform {
    view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
    flags: [f32; 4],
    light_positions: [[f32; 4]; 3],
    light_colors: [[f32; 4]; 3],
    run_min: [f32; 4],
    run_max: [f32; 4],
    hover_min: [f32; 4],
    hover_max: [f32; 4],
}

/// Where one model is drawn; mirrors `Placement` in `kitchen.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PlacementUniform {
    model: [[f32; 4]; 4],
    highlight: [f32; 4],
    bounds_min: [f32; 4],
    bounds_max: [f32; 4],
}

/// Per-material uniforms; mirrors `MaterialUniform` in `kitchen.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    params: [f32; 4],
    emissive: [f32; 4],
}

/// One primitive on the GPU: its buffers and the bind group of its material.
struct GpuPrimitive {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    material: wgpu::BindGroup,
    transparent: bool,
    /// Average vertex position in the model's own space, for depth sorting.
    centroid: Vec3,
}

/// A model uploaded once and drawn wherever it is placed.
struct GpuModel {
    primitives: Vec<GpuPrimitive>,
    /// Authored bounds, which is what lining the model up in a slot goes by.
    min: Vec3,
    max: Vec3,
}

/// A uniform buffer holding one placement, with the bind group over it.
struct Placement {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// What to draw in one slot this frame.
pub struct SlotView<'a> {
    /// The module in the slot, or `None` for the placeholder.
    pub module: Option<&'a str>,
    pub translation: Vec3,
    /// How lit up the slot is, 0 to 1.
    pub highlight: f32,
    /// The space the slot occupies, whose edges light up like LED strips.
    pub bounds: Aabb,
}

/// Owns the GPU resources for one canvas.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    sample_count: u32,
    /// Set when the surface format is not sRGB, so the shader encodes gamma.
    manual_gamma: bool,
    globals: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    environment_bind_group: wgpu::BindGroup,
    /// x: 1.0 when the skybox images are bound. y, z: their deepest mip levels.
    environment_flags: [f32; 3],
    opaque_pipeline: wgpu::RenderPipeline,
    transparent_pipeline: wgpu::RenderPipeline,
    /// Kept for uploading modules after start-up.
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white_view: wgpu::TextureView,
    light_positions: [[f32; 4]; 3],
    light_colors: [[f32; 4]; 3],
    run_min: [f32; 4],
    run_max: [f32; 4],
    /// Floor, wall and lamps: already in world space, drawn at the origin.
    room: Vec<GpuModel>,
    room_placement: Placement,
    placeholder: GpuModel,
    /// Modules by the id the web app gave them.
    modules: HashMap<String, GpuModel>,
    /// One placement per slot, rewritten every frame.
    slot_placements: Vec<Placement>,
    depth_view: wgpu::TextureView,
    msaa_view: Option<wgpu::TextureView>,
    backend: String,
}

impl Renderer {
    /// Set up a device for `canvas` and upload the room for `layout`, the
    /// placeholder and the skybox.
    ///
    /// `environment` is optional: without it the fronts reflect a procedural
    /// gradient rather than the constructor refusing to start. `width` and
    /// `height` are in physical pixels.
    pub async fn new(
        canvas: web_sys::HtmlCanvasElement,
        layout: &KitchenLayout,
        placeholder: &Model,
        environment: Option<&Environment>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let instance = wgpu::util::new_instance_with_webgpu_detection(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
        .await;

        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|error| format!("could not create a surface for the canvas: {error}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| format!("no GPU adapter is available in this browser: {error}"))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("kitchen device"),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("could not acquire a GPU device: {error}"))?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| "the surface exposes no texture formats".to_owned())?;

        let sample_count = if adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(PREFERRED_SAMPLE_COUNT)
        {
            PREFERRED_SAMPLE_COUNT
        } else {
            1
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: capabilities
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("kitchen shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("kitchen.wgsl"))),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let placement_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placement layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                sampler_entry(2),
            ],
        });
        let environment_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("environment layout"),
                entries: &[texture_entry(0), texture_entry(1), sampler_entry(2)],
            });

        // Four groups, which is exactly the WebGL2 downlevel limit.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("kitchen pipeline layout"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&placement_layout),
                Some(&material_layout),
                Some(&environment_layout),
            ],
            immediate_size: 0,
        });

        let opaque_pipeline = build_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            sample_count,
            Blending::Opaque,
        );
        let transparent_pipeline = build_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            format,
            sample_count,
            Blending::Alpha,
        );

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: core::mem::size_of::<GlobalsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });

        // Repeating, and anisotropic so the tiles stay crisp as the floor
        // recedes; wgpu drops the anisotropy where the backend cannot do it.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });

        let white = TextureData {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        let white_view = upload_texture(&device, &queue, &[white], "white fallback");

        let (environment_bind_group, environment_flags) =
            upload_environment(&device, &queue, &environment_layout, environment);

        let uploader = Uploader {
            device: &device,
            material_layout: &material_layout,
            sampler: &sampler,
            white_view: &white_view,
        };

        let room = build_room(&uploader, &queue, layout);
        let placeholder = uploader.model(&queue, placeholder, "placeholder");

        let room_placement = create_placement(&device, &placement_layout, "room");
        queue.write_buffer(
            &room_placement.buffer,
            0,
            bytemuck::bytes_of(&PlacementUniform {
                model: Mat4::IDENTITY.to_cols_array_2d(),
                highlight: [0.0; 4],
                bounds_min: [0.0; 4],
                bounds_max: [0.0; 4],
            }),
        );
        let slot_placements = (0..layout.slot_count())
            .map(|index| create_placement(&device, &placement_layout, &format!("slot {index}")))
            .collect();

        let light_color = LAMP_LIGHT_COLOR.map(|channel| channel * LAMP_INTENSITY);
        let lamps = layout.lamps();
        // The light sits just under the diffuser, so the shade stays dark on
        // the outside and lit on the inside.
        let light_positions = lamps.map(|lamp| {
            [
                lamp.light.x,
                lamp.light.y - 0.03,
                lamp.light.z,
                LAMP_RANGE_M,
            ]
        });
        let light_colors = [[light_color[0], light_color[1], light_color[2], 1.0]; 3];

        let slots = layout.slots();
        let run = match (slots.first(), slots.last()) {
            (Some(first), Some(last)) => Aabb::new(first.bounds.min, last.bounds.max),
            _ => Aabb::new(Vec3::ZERO, Vec3::ZERO),
        };
        let run_min = [run.min.x, run.min.y, run.min.z, layout.wall_width() * 0.5];
        let run_max = [run.max.x, run.max.y, run.max.z, layout.wall_height()];

        let depth_view = create_depth_view(&device, &config, sample_count);
        let msaa_view = create_msaa_view(&device, &config, sample_count);

        let backend = match adapter.get_info().backend {
            wgpu::Backend::BrowserWebGpu => "WebGPU".to_owned(),
            wgpu::Backend::Gl => "WebGL2".to_owned(),
            other => format!("{other:?}"),
        };

        Ok(Self {
            surface,
            device,
            queue,
            config,
            sample_count,
            manual_gamma: !format.is_srgb(),
            globals,
            globals_bind_group,
            environment_bind_group,
            environment_flags,
            opaque_pipeline,
            transparent_pipeline,
            material_layout,
            sampler,
            white_view,
            light_positions,
            light_colors,
            run_min,
            run_max,
            room,
            room_placement,
            placeholder,
            modules: HashMap::new(),
            slot_placements,
            depth_view,
            msaa_view,
            backend,
        })
    }

    /// Which backend wgpu ended up on, for the UI to display.
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Current drawing buffer size, in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Reconfigure for a new canvas size, in physical pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if (width, height) == (self.config.width, self.config.height) {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, &self.config, self.sample_count);
        self.msaa_view = create_msaa_view(&self.device, &self.config, self.sample_count);
    }

    /// Whether a module with this id has been uploaded already.
    pub fn has_module(&self, id: &str) -> bool {
        self.modules.contains_key(id)
    }

    /// Upload `model` under `id`, replacing anything uploaded under it before.
    pub fn add_module(&mut self, id: &str, model: &Model) {
        let uploader = Uploader {
            device: &self.device,
            material_layout: &self.material_layout,
            sampler: &self.sampler,
            white_view: &self.white_view,
        };
        let gpu_model = uploader.model(&self.queue, model, id);
        self.modules.insert(id.to_owned(), gpu_model);
    }

    /// Authored bounds of the module with this id, or of the placeholder.
    pub fn model_bounds(&self, module: Option<&str>) -> (Vec3, Vec3) {
        let model = self.resolve(module);
        (model.min, model.max)
    }

    /// The model to draw for a slot: the module if it has been uploaded, the
    /// placeholder otherwise.
    fn resolve(&self, module: Option<&str>) -> &GpuModel {
        module
            .and_then(|id| self.modules.get(id))
            .unwrap_or(&self.placeholder)
    }

    /// Draw one frame from `camera`'s point of view, with `slots` in order.
    pub fn render(&mut self, camera: &RoomCamera, slots: &[SlotView<'_>]) -> Result<(), String> {
        let eye = camera.eye();
        // The most lit-up slot spills its LED light onto the floor and wall.
        let (hover_min, hover_max) = slots
            .iter()
            .filter(|slot| slot.highlight > 0.0)
            .max_by(|a, b| a.highlight.total_cmp(&b.highlight))
            .map_or(([0.0; 4], [0.0; 4]), |slot| {
                (
                    slot.bounds
                        .min
                        .extend(slot.highlight.clamp(0.0, 1.0))
                        .to_array(),
                    slot.bounds.max.extend(0.0).to_array(),
                )
            });
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&GlobalsUniform {
                view_projection: camera.view_projection().to_cols_array_2d(),
                camera_position: [eye.x, eye.y, eye.z, 1.0],
                flags: [
                    if self.manual_gamma { 1.0 } else { 0.0 },
                    self.environment_flags[0],
                    self.environment_flags[1],
                    self.environment_flags[2],
                ],
                light_positions: self.light_positions,
                light_colors: self.light_colors,
                run_min: self.run_min,
                run_max: self.run_max,
                hover_min,
                hover_max,
            }),
        );

        for (slot, placement) in slots.iter().zip(&self.slot_placements) {
            let is_placeholder = slot.module.is_none_or(|id| !self.modules.contains_key(id));
            self.queue.write_buffer(
                &placement.buffer,
                0,
                bytemuck::bytes_of(&PlacementUniform {
                    model: Mat4::from_translation(slot.translation).to_cols_array_2d(),
                    highlight: [
                        slot.highlight.clamp(0.0, 1.0),
                        if is_placeholder { 1.0 } else { 0.0 },
                        0.0,
                        0.0,
                    ],
                    bounds_min: slot.bounds.min.extend(0.0).to_array(),
                    bounds_max: slot.bounds.max.extend(0.0).to_array(),
                }),
            );
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(())
            }
            other => return Err(format!("could not acquire a frame: {other:?}")),
        };

        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Everything placed this frame: the room at the origin, then each slot.
        let mut placed: Vec<(&GpuModel, &wgpu::BindGroup, Vec3)> = self
            .room
            .iter()
            .map(|model| (model, &self.room_placement.bind_group, Vec3::ZERO))
            .collect();
        for (slot, placement) in slots.iter().zip(&self.slot_placements) {
            placed.push((
                self.resolve(slot.module),
                &placement.bind_group,
                slot.translation,
            ));
        }

        let mut opaque = Vec::new();
        let mut transparent = Vec::new();
        for &(model, bind_group, translation) in &placed {
            for primitive in &model.primitives {
                if primitive.transparent {
                    let depth = (primitive.centroid + translation - eye).length_squared();
                    transparent.push((primitive, bind_group, depth));
                } else {
                    opaque.push((primitive, bind_group));
                }
            }
        }
        // Back to front, so the glass boxes composite over each other properly.
        transparent.sort_by(|a, b| b.2.total_cmp(&a.2));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("kitchen frame"),
            });

        {
            let (view, resolve_target) = match &self.msaa_view {
                Some(msaa) => (msaa, Some(&frame_view)),
                None => (&frame_view, None),
            };

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kitchen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color(self.manual_gamma)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(3, &self.environment_bind_group, &[]);

            pass.set_pipeline(&self.opaque_pipeline);
            for (primitive, placement) in opaque {
                draw_primitive(&mut pass, primitive, placement);
            }

            pass.set_pipeline(&self.transparent_pipeline);
            for (primitive, placement, _) in transparent {
                draw_primitive(&mut pass, primitive, placement);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}

fn draw_primitive(
    pass: &mut wgpu::RenderPass<'_>,
    primitive: &GpuPrimitive,
    placement: &wgpu::BindGroup,
) {
    pass.set_bind_group(1, placement, &[]);
    pass.set_bind_group(2, &primitive.material, &[]);
    pass.set_vertex_buffer(0, primitive.vertices.slice(..));
    pass.set_index_buffer(primitive.indices.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..primitive.index_count, 0, 0..1);
}

/// What uploading a mesh or a model needs, bundled so it works both while the
/// renderer is being built and afterwards.
struct Uploader<'a> {
    device: &'a wgpu::Device,
    material_layout: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    white_view: &'a wgpu::TextureView,
}

impl Uploader<'_> {
    fn material(
        &self,
        uniform: MaterialUniform,
        texture: Option<&wgpu::TextureView>,
        label: &str,
    ) -> wgpu::BindGroup {
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(
                        texture.unwrap_or(self.white_view),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(self.sampler),
                },
            ],
        })
    }

    fn primitive(
        &self,
        vertices: &[Vertex],
        indices: &[u32],
        material: wgpu::BindGroup,
        transparent: bool,
        centroid: Vec3,
        label: &str,
    ) -> GpuPrimitive {
        GpuPrimitive {
            vertices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("{label} vertices")),
                    contents: bytemuck::cast_slice(vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            indices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("{label} indices")),
                    contents: bytemuck::cast_slice(indices),
                    usage: wgpu::BufferUsages::INDEX,
                }),
            index_count: indices.len() as u32,
            material,
            transparent,
            centroid,
        }
    }

    /// A procedural mesh with a single material.
    fn mesh(&self, mesh: &Mesh, material: wgpu::BindGroup, label: &str) -> GpuModel {
        let bounds = mesh.bounds();
        GpuModel {
            primitives: vec![self.primitive(
                &mesh.vertices,
                &mesh.indices,
                material,
                false,
                bounds.center(),
                label,
            )],
            min: bounds.min,
            max: bounds.max,
        }
    }

    /// A glTF model, with its textures and one material per primitive.
    fn model(&self, queue: &wgpu::Queue, model: &Model, label: &str) -> GpuModel {
        let texture_views: Vec<wgpu::TextureView> = model
            .textures
            .iter()
            .enumerate()
            .map(|(index, texture)| {
                upload_texture(
                    self.device,
                    queue,
                    core::slice::from_ref(texture),
                    &format!("{label} texture {index}"),
                )
            })
            .collect();

        let primitives = model
            .primitives
            .iter()
            .filter(|primitive| !primitive.indices.is_empty())
            .map(|primitive| {
                let material = &primitive.material;
                let texture = material
                    .base_color_texture
                    .and_then(|index| texture_views.get(index));
                let name = format!("{label} {}", primitive.name);

                let bind_group = self.material(
                    MaterialUniform {
                        base_color: material.base_color,
                        params: [
                            material.metallic,
                            material.roughness,
                            if texture.is_some() { 1.0 } else { 0.0 },
                            material.opacity(),
                        ],
                        emissive: [0.0, 0.0, 0.0, SURFACE_OTHER],
                    },
                    texture,
                    &name,
                );

                self.primitive(
                    &primitive.vertices,
                    &primitive.indices,
                    bind_group,
                    material.is_transparent(),
                    Vec3::from(primitive.centroid),
                    &name,
                )
            })
            .collect();

        let (min, max) = model.bounds();
        GpuModel {
            primitives,
            min,
            max,
        }
    }
}

/// The floor, the wall and the three lamps.
fn build_room(
    uploader: &Uploader<'_>,
    queue: &wgpu::Queue,
    layout: &KitchenLayout,
) -> Vec<GpuModel> {
    let floor_levels = floor::tile_texture(FLOOR_TEXTURE_SIZE);
    let floor_view = upload_texture(uploader.device, queue, &floor_levels, "floor tiles");
    let floor_material = uploader.material(
        MaterialUniform {
            base_color: [1.0; 4],
            params: [0.0, 0.32, 1.0, 1.0],
            emissive: [0.0, 0.0, 0.0, SURFACE_FLOOR],
        },
        Some(&floor_view),
        "floor",
    );
    let floor_mesh = geometry::cuboid(layout.floor_bounds(), floor::TEXTURE_PERIOD_M);

    let wall_material = uploader.material(
        MaterialUniform {
            base_color: linear_color(WALL_COLOR),
            params: [0.0, 0.9, 0.0, 1.0],
            emissive: [0.0, 0.0, 0.0, SURFACE_WALL],
        },
        None,
        "wall",
    );
    let wall_mesh = geometry::cuboid(layout.wall_bounds(), 1.0);

    let mut lamp_bodies = Mesh::default();
    let mut diffusers = Mesh::default();
    for lamp in layout.lamps() {
        let shade_top = lamp.light + Vec3::Y * SHADE_HEIGHT;
        lamp_bodies.append(&geometry::cone_shell(
            shade_top,
            SHADE_TOP_RADIUS,
            SHADE_BOTTOM_RADIUS,
            SHADE_HEIGHT,
            LAMP_SEGMENTS,
        ));
        lamp_bodies.append(&geometry::cuboid(
            Aabb::new(
                shade_top - Vec3::new(CORD_HALF_WIDTH, 0.0, CORD_HALF_WIDTH),
                lamp.ceiling + Vec3::new(CORD_HALF_WIDTH, 0.0, CORD_HALF_WIDTH),
            ),
            1.0,
        ));
        lamp_bodies.append(&geometry::cuboid(
            Aabb::new(
                lamp.ceiling - Vec3::new(ROSE_HALF_SIZE, ROSE_HEIGHT, ROSE_HALF_SIZE),
                lamp.ceiling + Vec3::new(ROSE_HALF_SIZE, 0.0, ROSE_HALF_SIZE),
            ),
            1.0,
        ));

        // Tucked a little up inside the rim, where the cone is narrower.
        let inset = 0.02;
        let radius =
            SHADE_BOTTOM_RADIUS - (SHADE_BOTTOM_RADIUS - SHADE_TOP_RADIUS) * inset / SHADE_HEIGHT;
        diffusers.append(&geometry::disc_facing_down(
            lamp.light + Vec3::Y * inset,
            radius * 0.98,
            LAMP_SEGMENTS,
        ));
    }

    let lamp_material = uploader.material(
        MaterialUniform {
            base_color: linear_color(LAMP_BODY_COLOR),
            params: [0.4, 0.45, 0.0, 1.0],
            emissive: [0.0, 0.0, 0.0, SURFACE_OTHER],
        },
        None,
        "lamp bodies",
    );
    let diffuser_material = uploader.material(
        MaterialUniform {
            base_color: [1.0; 4],
            params: [0.0, 0.9, 0.0, 1.0],
            emissive: [
                DIFFUSER_EMISSION[0],
                DIFFUSER_EMISSION[1],
                DIFFUSER_EMISSION[2],
                SURFACE_OTHER,
            ],
        },
        None,
        "lamp diffusers",
    );

    vec![
        uploader.mesh(&floor_mesh, floor_material, "floor"),
        uploader.mesh(&wall_mesh, wall_material, "wall"),
        uploader.mesh(&lamp_bodies, lamp_material, "lamp bodies"),
        uploader.mesh(&diffusers, diffuser_material, "lamp diffusers"),
    ]
}

fn create_placement(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    label: &str,
) -> Placement {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: core::mem::size_of::<PlacementUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });

    Placement { buffer, bind_group }
}

/// Bind the skybox, or a grey stand-in plus a flag telling the shader to use
/// its gradient.
fn upload_environment(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    environment: Option<&Environment>,
) -> (wgpu::BindGroup, [f32; 3]) {
    // Repeat across, clamp down: the panorama wraps around the horizon but has a
    // top and a bottom.
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("environment sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });

    let (background, foreground, flags) = match environment {
        Some(environment) => (
            upload_environment_map(device, queue, &environment.background, "sky background"),
            upload_environment_map(device, queue, &environment.foreground, "sky foreground"),
            [
                1.0,
                environment.background.max_lod(),
                environment.foreground.max_lod(),
            ],
        ),
        None => {
            let grey = TextureData {
                width: 1,
                height: 1,
                rgba: vec![128, 128, 128, 255],
            };
            (
                upload_texture(
                    device,
                    queue,
                    core::slice::from_ref(&grey),
                    "sky background fallback",
                ),
                upload_texture(device, queue, &[grey], "sky foreground fallback"),
                [0.0, 0.0, 0.0],
            )
        }
    };

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("environment"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&background),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&foreground),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    (bind_group, flags)
}

/// How a pipeline writes to the colour and depth targets.
enum Blending {
    /// Writes depth, no blending.
    Opaque,
    /// Blends over what is already there and leaves depth alone, so overlapping
    /// glass does not hide the glass behind it.
    Alpha,
}

fn build_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    sample_count: u32,
    blending: Blending,
) -> wgpu::RenderPipeline {
    let transparent = matches!(blending, Blending::Alpha);

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if transparent {
            "transparent pipeline"
        } else {
            "opaque pipeline"
        }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: core::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, // position
                    1 => Float32x3, // normal
                    2 => Float32x2, // uv
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: transparent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            // The glTF assets are exported double sided and the lamp shade is
            // seen from inside and out, so nothing is culled; the shader flips
            // normals that face away from the camera.
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!transparent),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

/// The clear colour, in whatever space the surface expects - see the note on
/// `clear_color` in `wasm-viewer`'s renderer.
fn clear_color(manual_gamma: bool) -> wgpu::Color {
    let convert = |value: f64| {
        if manual_gamma {
            value
        } else {
            srgb_to_linear(value as f32) as f64
        }
    };

    wgpu::Color {
        r: convert(BACKGROUND[0]),
        g: convert(BACKGROUND[1]),
        b: convert(BACKGROUND[2]),
        a: 1.0,
    }
}

/// An opaque sRGB colour as the linear base colour factor the shader takes.
fn linear_color(srgb: [f32; 3]) -> [f32; 4] {
    [
        srgb_to_linear(srgb[0]),
        srgb_to_linear(srgb[1]),
        srgb_to_linear(srgb[2]),
        1.0,
    ]
}

/// The sRGB electro-optical transfer function.
fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn create_depth_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    sample_count: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_msaa_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    sample_count: u32,
) -> Option<wgpu::TextureView> {
    if sample_count <= 1 {
        return None;
    }

    Some(
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("msaa colour"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default()),
    )
}

/// Upload an sRGB texture with the mip levels given, level 0 first.
fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    levels: &[TextureData],
    label: &str,
) -> wgpu::TextureView {
    let base = &levels[0];
    let packed: Vec<u8> = levels
        .iter()
        .flat_map(|level| level.rgba.iter().copied())
        .collect();

    device
        .create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: base.width,
                    height: base.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::MipMajor,
            &packed,
        )
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn upload_environment_map(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    map: &EnvironmentMap,
    label: &str,
) -> wgpu::TextureView {
    upload_texture(device, queue, &map.levels, label)
}
