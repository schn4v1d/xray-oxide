use cgmath::SquareMatrix;
use hassle_rs::{Dxc, DxcIncludeHandler, HassleError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};
use xray_oxide_core::filesystem::Filesystem;
use xray_oxide_render::Renderer;

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("No compatible GPU found")]
    NoGPUFound,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 4],
    tex_coords: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2, 2 => Float32x4];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct DynamicTransforms {
    matrix_world_view_projection: [[f32; 4]; 4],
    matrix_world_view: [[f32; 4]; 4],
    matrix_world: [[f32; 4]; 4],

    /// ?
    L_material: [f32; 4],
    hemi_cube_pos_faces: [f32; 4],
    hemi_cube_neg_faces: [f32; 4],
    /// ?
    dt_params: [f32; 4],
}

impl DynamicTransforms {
    fn new() -> DynamicTransforms {
        DynamicTransforms {
            matrix_world_view_projection: cgmath::Matrix4::identity().into(),
            matrix_world_view: cgmath::Matrix4::identity().into(),
            matrix_world: cgmath::Matrix4::identity().into(),

            L_material: [0.0; 4],
            hemi_cube_pos_faces: [0.0; 4],
            hemi_cube_neg_faces: [0.0; 4],
            dt_params: [0.0; 4],
        }
    }
}

pub struct WgpuRenderer {
    filesystem: Arc<Filesystem>,
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    window: Window,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    dynamic_transforms: DynamicTransforms,
    dynamic_transforms_buffer: wgpu::Buffer,
    dynamic_transforms_bind_group: wgpu::BindGroup,
}

impl WgpuRenderer {
    pub async fn new(window: Window, filesystem: Arc<Filesystem>) -> anyhow::Result<WgpuRenderer> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            // flags: Default::default(),
            dx12_shader_compiler: Default::default(),
            // gles_minor_version: Default::default(),
        });

        let surface = unsafe { instance.create_surface(&window)? };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RendererError::NoGPUFound)?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);

        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&device, &config);

        let texture_size = wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        };

        let diffuse_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("texture"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &diffuse_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[0, 0, 0, 0],
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            texture_size,
        );

        let diffuse_texture_view =
            diffuse_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let diffuse_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let dynamic_transforms = DynamicTransforms::new();

        let dynamic_transforms_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Dynamic Transforms"),
                contents: bytemuck::cast_slice(&[dynamic_transforms]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 6,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 7,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                ],
            });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: dynamic_transforms_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::Sampler(&diffuse_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture_view),
                },
            ],
        });

        let vertex_shader_module =
            create_vertex_shader_module(&device, &filesystem, "stub_default")?;

        let fragment_shader_module =
            create_fragment_shader_module(&device, &filesystem, "stub_default")?;

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = create_render_pipeline(
            &device,
            Some("Render Pipeline"),
            &render_pipeline_layout,
            (&vertex_shader_module, &[Vertex::desc()]),
            (
                &fragment_shader_module,
                &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            ),
        );

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: &[],
            usage: wgpu::BufferUsages::VERTEX
        });

        Ok(WgpuRenderer {
            filesystem,
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            vertex_buffer,
            dynamic_transforms,
            dynamic_transforms_buffer,
            dynamic_transforms_bind_group: bind_group,
        })
    }

    fn render_impl(&mut self) -> anyhow::Result<()> {
        let output = self.surface.get_current_texture()?;

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: true,
                    },
                })],
                depth_stencil_attachment: None,
                // timestamp_writes: None,
                // occlusion_query_set: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_bind_group(0, &self.dynamic_transforms_bind_group, &[]);
            render_pass.draw(0..0, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

pub struct ShaderModule {
    pub module: wgpu::ShaderModule,
    pub entry_point: String,
}

struct IncludeHandler<'a> {
    filesystem: &'a Filesystem,
}

impl<'a> DxcIncludeHandler for IncludeHandler<'a> {
    fn load_source(&mut self, filename: String) -> Option<String> {
        let filename = if cfg!(windows) {
            filename.replace('/', "\\")
        } else {
            filename
        };

        self.filesystem.read_to_string(filename).ok()
    }
}

fn create_shader_module<P: AsRef<Path>, F: Fn(&str) -> (&str, &str)>(
    device: &wgpu::Device,
    filesystem: &Filesystem,
    shader_path: P,
    get_params: F,
) -> anyhow::Result<ShaderModule> {
    let mut path = filesystem.append_path("$game_shaders$", "r3").unwrap();
    path.push(shader_path.as_ref());

    let shader_code = filesystem.read_to_string(&path)?;

    let (target_profile, entry_point) = get_params(&shader_code);

    let dxc = Dxc::new(None)?;

    let compiler = dxc.create_compiler()?;
    let library = dxc.create_library()?;

    let blob = library.create_blob_with_encoding_from_str(&shader_code)?;

    let spirv = match compiler.compile(
        &blob,
        path.to_str().unwrap(),
        entry_point,
        target_profile,
        &["-spirv", "-Zs", "-Gec"],
        Some(&mut IncludeHandler { filesystem }),
        &[],
    ) {
        Err(result) => {
            let error_blob = result.0.get_error_buffer()?;
            Err(HassleError::CompileError(
                library.get_blob_as_string(&error_blob.into())?,
            ))
        }
        Ok(result) => {
            let result_blob = result.get_result()?;

            Ok(result_blob.to_vec())
        }
    }?;

    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(
        path.with_file_name(path.file_name().unwrap().to_str().unwrap().to_owned() + ".spirv"),
        &spirv,
    )?;

    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: shader_path.as_ref().to_str(),
        source: wgpu::util::make_spirv(&spirv),
    });

    Ok(ShaderModule {
        module,
        entry_point: entry_point.to_owned(),
    })
}

fn create_vertex_shader_module<P: AsRef<Path>>(
    device: &wgpu::Device,
    filesystem: &Filesystem,
    shader_path: P,
) -> anyhow::Result<ShaderModule> {
    create_shader_module(
        device,
        filesystem,
        shader_path.as_ref().with_extension("vs"),
        |code| {
            let entry = if code.contains("main_vs_1_1") {
                "main_vs_1_1"
            } else if code.contains("main_vs_2_0") {
                "main_vs_2_0"
            } else {
                "main"
            };

            ("vs_5_0", entry)
        },
    )
}

fn create_fragment_shader_module<P: AsRef<Path>>(
    device: &wgpu::Device,
    filesystem: &Filesystem,
    shader_path: P,
) -> anyhow::Result<ShaderModule> {
    create_shader_module(
        device,
        filesystem,
        shader_path.as_ref().with_extension("ps"),
        |code| {
            let entry = if code.contains("main_ps_1_1") {
                "main_ps_1_1"
            } else if code.contains("main_ps_1_2") {
                "main_ps_1_2"
            } else if code.contains("main_ps_1_3") {
                "main_ps_1_3"
            } else if code.contains("main_ps_1_4") {
                "main_ps_1_4"
            } else if code.contains("main_ps_2_0") {
                "main_ps_2_0"
            } else {
                "main"
            };

            ("ps_5_0", entry)
        },
    )
}

fn create_render_pipeline(
    device: &wgpu::Device,
    label: Option<&str>,
    layout: &wgpu::PipelineLayout,
    vertex: (&ShaderModule, &[wgpu::VertexBufferLayout]),
    fragment: (&ShaderModule, &[Option<wgpu::ColorTargetState>]),
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label,
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &vertex.0.module,
            entry_point: &vertex.0.entry_point,
            buffers: vertex.1,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: &fragment.0.module,
            entry_point: &fragment.0.entry_point,
            targets: fragment.1,
        }),
        multiview: None,
    })
}

impl Renderer for WgpuRenderer {
    fn window(&self) -> &Window {
        &self.window
    }

    fn resize(&mut self, new_size: Option<PhysicalSize<u32>>) {
        let new_size = new_size.unwrap_or(self.size);
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn render(&mut self) -> anyhow::Result<()> {
        match self.render_impl() {
            Ok(_) => Ok(()),
            Err(e) => match e.downcast_ref::<wgpu::SurfaceError>() {
                Some(wgpu::SurfaceError::Lost) => {
                    self.resize(None);
                    Ok(())
                }
                Some(wgpu::SurfaceError::OutOfMemory) => Err(e),
                Some(e) => {
                    eprintln!("{e:?}");
                    Ok(())
                }
                _ => Err(e),
            },
        }
    }
}
