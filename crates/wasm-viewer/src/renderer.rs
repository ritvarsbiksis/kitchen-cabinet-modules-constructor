//! The wgpu side of the viewer: device setup, pipelines and the draw loop.
//!
//! Only compiled for `wasm32`, since it renders into an HTML canvas. wgpu picks
//! WebGPU when the browser exposes it and falls back to WebGL2 otherwise, which
//! is why the limits requested here stay inside the WebGL2 downlevel defaults.

use std::borrow::Cow;

use glam::Vec3;
use wgpu::util::DeviceExt;

use crate::camera::OrbitCamera;
use crate::environment::{Environment, EnvironmentMap};
use crate::model::{Material, Model, TextureData, Vertex};

/// Multisampling level to ask for. The frame of a pair of glasses is mostly thin
/// diagonal edges, which alias badly without it.
const PREFERRED_SAMPLE_COUNT: u32 = 4;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
/// Background of the canvas, as sRGB components - the values the display should
/// actually show. [`clear_color`] converts them for whichever target is in use.
/// Only visible when the skybox images are missing; otherwise the skybox pass
/// paints over every pixel of it.
const BACKGROUND: [f64; 3] = [0.075, 0.079, 0.091];

/// Uniforms shared by every draw in a frame. `repr(C)` plus `Pod` so it can go
/// to the GPU as raw bytes; the layout mirrors `Globals` in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlobalsUniform {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
    flags: [f32; 4],
}

/// Per-material uniforms; mirrors `MaterialUniform` in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MaterialUniform {
    base_color: [f32; 4],
    params: [f32; 4],
}

impl MaterialUniform {
    fn new(material: &Material, has_texture: bool) -> Self {
        Self {
            base_color: material.base_color,
            params: [
                material.metallic,
                material.roughness,
                if has_texture { 1.0 } else { 0.0 },
                material.opacity(),
            ],
        }
    }
}

/// One primitive, ready to draw.
struct Draw {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    material_bind_group: wgpu::BindGroup,
    texture_bind_group: wgpu::BindGroup,
    transparent: bool,
    centroid: Vec3,
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
    opaque_pipeline: wgpu::RenderPipeline,
    transparent_pipeline: wgpu::RenderPipeline,
    /// Draws the backdrop before anything else in the frame.
    sky_pipeline: wgpu::RenderPipeline,
    environment_bind_group: wgpu::BindGroup,
    /// Bound for the skybox draw, which shares a pipeline layout with the model
    /// but uses neither of these groups.
    placeholder_material_bind_group: wgpu::BindGroup,
    placeholder_texture_bind_group: wgpu::BindGroup,
    /// x: 1.0 when the skybox images are bound. y, z: their deepest mip levels.
    environment_flags: [f32; 3],
    depth_view: wgpu::TextureView,
    /// The multisampled colour target, absent when running without MSAA.
    msaa_view: Option<wgpu::TextureView>,
    draws: Vec<Draw>,
    /// Description of the adapter, surfaced in the UI.
    backend: String,
}

impl Renderer {
    /// Set up a device for `canvas` and upload `model` and `environment`.
    ///
    /// `environment` is optional: when the skybox images could not be fetched or
    /// decoded, the shader falls back to its procedural gradient rather than the
    /// viewer refusing to start.
    ///
    /// `width` and `height` are in physical pixels.
    pub async fn new(
        canvas: web_sys::HtmlCanvasElement,
        model: &Model,
        environment: Option<&Environment>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // Prefer WebGPU, but keep the GL backend enabled so browsers without it
        // still get a picture. The helper drops WebGPU from the list when
        // `navigator.gpu` is missing, which avoids a failed adapter request.
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
                label: Some("viewer device"),
                // Staying inside the WebGL2 downlevel defaults keeps the same
                // code working on both backends.
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|error| format!("could not acquire a GPU device: {error}"))?;

        let capabilities = surface.get_capabilities(&adapter);
        // An sRGB surface lets the hardware do the encoding; otherwise the
        // shader has to, which `manual_gamma` switches on.
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
            label: Some("viewer shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture layout"),
            entries: &[
                texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // The skybox: the panorama, the light card and the one sampler they share.
        let environment_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("environment layout"),
                entries: &[
                    texture_entry(0),
                    texture_entry(1),
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer pipeline layout"),
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&material_layout),
                Some(&texture_layout),
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
        let sky_pipeline =
            build_sky_pipeline(&device, &pipeline_layout, &shader, format, sample_count);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("base colour sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // Materials without a base colour map still go through the same bind
        // group layout, so they get a 1x1 white texture instead of a second
        // pipeline and shader variant.
        let white = TextureData {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        };
        let fallback_view = upload_texture(&device, &queue, &white, "white fallback");

        // Repeat across, clamp down: the panorama wraps all the way around the
        // horizon but has a top and a bottom. Linear between mip levels, which is
        // how roughness fades a reflection from sharp to soft.
        let environment_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // Without the images the bindings still have to be filled, so a single
        // grey texel goes in and `environment_flags` tells the shader to use its
        // gradient instead.
        let (background_view, foreground_view, environment_flags) = match environment {
            Some(environment) => (
                upload_environment_map(&device, &queue, &environment.background, "sky background"),
                upload_environment_map(&device, &queue, &environment.foreground, "sky foreground"),
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
                    upload_texture(&device, &queue, &grey, "sky background fallback"),
                    upload_texture(&device, &queue, &grey, "sky foreground fallback"),
                    [0.0, 0.0, 0.0],
                )
            }
        };

        let environment_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment"),
            layout: &environment_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&background_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&foreground_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&environment_sampler),
                },
            ],
        });

        // The skybox draw goes through the model's pipeline layout, so groups 1
        // and 2 have to be filled even though `fs_sky` never reads them.
        let placeholder_material = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("placeholder material"),
            contents: bytemuck::bytes_of(&MaterialUniform {
                base_color: [1.0; 4],
                params: [0.0, 1.0, 0.0, 1.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let placeholder_material_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("placeholder material"),
                layout: &material_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: placeholder_material.as_entire_binding(),
                }],
            });
        let placeholder_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("placeholder texture"),
            layout: &texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&fallback_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let texture_views: Vec<wgpu::TextureView> = model
            .textures
            .iter()
            .enumerate()
            .map(|(index, texture)| {
                upload_texture(&device, &queue, texture, &format!("gltf texture {index}"))
            })
            .collect();

        let draws = model
            .primitives
            .iter()
            .filter(|primitive| !primitive.indices.is_empty())
            .map(|primitive| {
                let texture_view = primitive
                    .material
                    .base_color_texture
                    .and_then(|index| texture_views.get(index))
                    .unwrap_or(&fallback_view);
                let has_texture = primitive
                    .material
                    .base_color_texture
                    .is_some_and(|index| index < texture_views.len());

                let material_uniform =
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("{} material", primitive.name)),
                        contents: bytemuck::bytes_of(&MaterialUniform::new(
                            &primitive.material,
                            has_texture,
                        )),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

                Draw {
                    vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("{} vertices", primitive.name)),
                        contents: bytemuck::cast_slice(&primitive.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("{} indices", primitive.name)),
                        contents: bytemuck::cast_slice(&primitive.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    index_count: primitive.indices.len() as u32,
                    material_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("{} material", primitive.name)),
                        layout: &material_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: material_uniform.as_entire_binding(),
                        }],
                    }),
                    texture_bind_group: device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("{} texture", primitive.name)),
                        layout: &texture_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(texture_view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&sampler),
                            },
                        ],
                    }),
                    transparent: primitive.material.is_transparent(),
                    centroid: Vec3::from(primitive.centroid),
                }
            })
            .collect();

        let depth_view = create_depth_view(&device, &config, sample_count);
        let msaa_view = create_msaa_view(&device, &config, sample_count);

        let info = adapter.get_info();
        let backend = match info.backend {
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
            opaque_pipeline,
            transparent_pipeline,
            sky_pipeline,
            environment_bind_group,
            placeholder_material_bind_group,
            placeholder_texture_bind_group,
            environment_flags,
            depth_view,
            msaa_view,
            draws,
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

    /// Draw one frame from `camera`'s point of view.
    pub fn render(&mut self, camera: &OrbitCamera) -> Result<(), String> {
        let eye = camera.eye();
        self.queue.write_buffer(
            &self.globals,
            0,
            bytemuck::bytes_of(&GlobalsUniform {
                view_projection: camera.view_projection().to_cols_array_2d(),
                inverse_view_projection: camera.view_projection().inverse().to_cols_array_2d(),
                camera_position: [eye.x, eye.y, eye.z, 1.0],
                flags: [
                    if self.manual_gamma { 1.0 } else { 0.0 },
                    self.environment_flags[0],
                    self.environment_flags[1],
                    self.environment_flags[2],
                ],
            }),
        );

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // A resize or a tab coming back from the background can invalidate
            // the swapchain; reconfiguring and skipping the frame recovers.
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

        // Back-to-front, so the blended lenses composite in the right order.
        let mut order: Vec<usize> = (0..self.draws.len()).collect();
        order.sort_by(|&a, &b| {
            let draw_a = &self.draws[a];
            let draw_b = &self.draws[b];
            draw_a.transparent.cmp(&draw_b.transparent).then_with(|| {
                (draw_b.centroid - eye)
                    .length_squared()
                    .total_cmp(&(draw_a.centroid - eye).length_squared())
            })
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("viewer frame"),
            });

        {
            // With MSAA the pass renders into the multisampled texture and
            // resolves into the frame; without it, straight into the frame.
            let (view, resolve_target) = match &self.msaa_view {
                Some(msaa) => (msaa, Some(&frame_view)),
                None => (&frame_view, None),
            };

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewer pass"),
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

            // The backdrop first, filling the frame at the far plane without
            // writing depth, so everything below simply draws over it. Skipped
            // without the images: there is nothing to draw but the single grey
            // texel standing in for them, and the clear colour is a better
            // backdrop than that.
            if self.environment_flags[0] > 0.5 {
                pass.set_pipeline(&self.sky_pipeline);
                pass.set_bind_group(1, &self.placeholder_material_bind_group, &[]);
                pass.set_bind_group(2, &self.placeholder_texture_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }

            for index in order {
                let draw = &self.draws[index];
                pass.set_pipeline(if draw.transparent {
                    &self.transparent_pipeline
                } else {
                    &self.opaque_pipeline
                });
                pass.set_bind_group(1, &draw.material_bind_group, &[]);
                pass.set_bind_group(2, &draw.texture_bind_group, &[]);
                pass.set_vertex_buffer(0, draw.vertices.slice(..));
                pass.set_index_buffer(draw.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..draw.index_count, 0, 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}

/// How a pipeline writes to the colour and depth targets.
enum Blending {
    /// Writes depth, no blending.
    Opaque,
    /// Blends over what is already there and leaves depth alone, so overlapping
    /// transparent surfaces do not hide each other.
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
            // The lenses and the frame are thin shells seen from both sides, so
            // nothing is culled and the shader flips back-facing normals.
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

/// The backdrop pipeline: one triangle built from the vertex index, so it needs
/// no vertex buffer, and no depth work at all - it is drawn first and everything
/// else is drawn over it.
fn build_sky_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sky pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_sky"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_sky"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            // The triangle sits on the far plane, which `Less` against a depth
            // buffer cleared to 1.0 would reject outright.
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Always),
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

/// The clear colour, in whatever space the surface expects.
///
/// wgpu passes the clear colour to the target untouched, so an sRGB surface
/// encodes it on write and needs the linear value, while a plain UNORM surface
/// (the one the shader is gamma-encoding for itself) takes the sRGB value as it
/// is. Without this the two backends show visibly different backgrounds.
fn clear_color(manual_gamma: bool) -> wgpu::Color {
    let convert = |value: f64| {
        if manual_gamma {
            value
        } else {
            srgb_to_linear(value)
        }
    };

    wgpu::Color {
        r: convert(BACKGROUND[0]),
        g: convert(BACKGROUND[1]),
        b: convert(BACKGROUND[2]),
        a: 1.0,
    }
}

/// The sRGB electro-optical transfer function.
fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
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

fn upload_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    data: &TextureData,
    label: &str,
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: data.width,
        height: data.height,
        depth_or_array_layers: 1,
    };

    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Base colour maps are authored in sRGB, so let the sampler linearise.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data.rgba,
    );

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Upload one skybox image with the mip chain `environment.rs` built for it. The
/// shader picks a level per pixel from the surface roughness, which is what makes
/// a rough reflection blurry and a polished one sharp.
fn upload_environment_map(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    map: &EnvironmentMap,
    label: &str,
) -> wgpu::TextureView {
    let (width, height) = map.size();

    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: map.levels.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Photographs, so sRGB in and linear light out of the sampler.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::MipMajor,
        &map.packed(),
    );

    texture.create_view(&wgpu::TextureViewDescriptor::default())
}
