use thiserror::Error;
use winit::{dpi::PhysicalSize, window::Window};
use xray_oxide_render::Renderer;

#[derive(Debug, Error)]
pub enum RendererError {
    #[error("No compatible GPU found")]
    NoGPUFound,
}

pub struct WgpuRenderer {
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    window: Window,
}

impl WgpuRenderer {
    pub async fn new(window: Window) -> anyhow::Result<WgpuRenderer> {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: Default::default(),
            dx12_shader_compiler: Default::default(),
            gles_minor_version: Default::default(),
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

        let text = r#"
            #include "common.h"
#include "shared\cloudconfig.h"

struct 	vi
{
	float4	p		: POSITION;
	float4	dir		: COLOR0;	// dir0,dir1(w<->z)
	float4	color	: COLOR1;	// rgb. intensity
};

struct 	vf
{
	float4	color	: COLOR0;	// rgb. intensity, for SM3 - tonemap-prescaled, HI-res
  	float2	tc0		: TEXCOORD0;
  	float2	tc1		: TEXCOORD1;
	float4 	hpos	: SV_Position;
};

vf main (vi v)
{
	vf 		o;

	o.hpos 		= mul		(m_WVP, v.p);	// xform, input in world coords

	// generate tcs
	float2  d0	= v.dir.xy*2-1;
	float2  d1	= v.dir.wz*2-1;
	float2 	_0	= v.p.xz * CLOUD_TILE0 + d0*timers.z*CLOUD_SPEED0;
	float2 	_1	= v.p.xz * CLOUD_TILE1 + d1*timers.z*CLOUD_SPEED1;
	o.tc0		= _0;					// copy tc
	o.tc1		= _1;					// copy tc

	o.color		=	v.color	;			// copy color, low precision, cannot prescale even by 2
	o.color.w	*= 	pow		(v.p.y,25);

	float	scale	= s_tonemap.Load( int3(0,0,0) ).x;

	o.color.rgb 	*= 	scale	;		// high precision

	return o;
}
        "#;

        match hassle_rs::compile_hlsl(
            r"G:\Anomaly\tools\_unpacked\shaders\r3\clouds.vs",
            text,
            "main",
            "vs_6_0",
            &["-spirv"],
            &[],
        ) {
            Ok(code) => {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("shader"),
                    source: wgpu::util::make_spirv(&code),
                });
            }
            Err(e) => {
                log::error!("{}", e);
            }
        }

        Ok(WgpuRenderer {
            window,
            surface,
            device,
            queue,
            config,
            size,
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
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
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
