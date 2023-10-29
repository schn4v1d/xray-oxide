use std::sync::Arc;
use std::thread;
use std::time::Instant;

use simple_logger::SimpleLogger;
use winit::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::{Window, WindowBuilder},
};
use xray_oxide_core::filesystem::Filesystem;
use xray_oxide_render::Renderer;
use xray_oxide_render_wgpu::WgpuRenderer;

use crate::ext::WindowExt;

pub mod ext;
pub mod splash;

pub fn entry_point() -> anyhow::Result<()> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        // .with_module_level("xray_oxide::core::filesystem", log::LevelFilter::Trace)
        .init()?;

    let start = Instant::now();

    let mut event_loop = EventLoop::new()?;

    let window = WindowBuilder::new()
        .with_title("XRay Oxide")
        .with_inner_size(LogicalSize::new(1920.0, 1080.0))
        .with_resizable(false)
        .with_visible(false)
        .build(&event_loop)
        .unwrap();

    let prepare_thread = {
        let proxy = event_loop.create_proxy();

        thread::spawn(move || -> anyhow::Result<XRay> {
            {
                let proxy = proxy.clone();
                thread_local_panic_hook::update_hook(Box::new(
                    move |old: &(dyn Fn(&std::panic::PanicInfo) + Send + Sync + 'static),
                          info: &std::panic::PanicInfo| {
                        proxy.send_event(()).unwrap();
                        old(info);
                    },
                ));
            }

            let app = XRay::new(window);
            proxy.send_event(()).unwrap();
            app
        })
    };

    splash::show_splash(&mut event_loop)?;

    let mut xray = prepare_thread.join().unwrap()?;

    let duration = start.elapsed();

    log::debug!("Created Application in {} seconds", duration.as_secs_f64());

    {
        let window = xray.renderer.window();
        window.center_window(&event_loop);
        window.set_visible(true);
        window.focus_window();
    }

    event_loop.run(move |event, target| {
        if let Event::WindowEvent { event, window_id } = event {
            if window_id == xray.renderer.window().id() {
                match event {
                    WindowEvent::CloseRequested => {
                        target.exit();
                    }
                    WindowEvent::RedrawRequested => {
                        if let Err(_) = xray.renderer.render() {
                            target.exit();
                        }
                    }
                    WindowEvent::Resized(new_size) => {
                        xray.renderer.resize(Some(new_size));
                    }
                    _ => {}
                }
            }
        }
    })?;

    Ok(())
}

pub struct LevelInfo {
    folder: String,
    name: String,
}

pub struct XRay {
    loaded: bool,
    ll_dwReference: u32,
    max_load_stage: u32,
    pub levels: Vec<LevelInfo>,
    pub current_level: Option<usize>,
    loading_screen: Option<()>,
    filesystem: Arc<Filesystem>,
    renderer: Box<dyn Renderer + Send>,
}

impl XRay {
    pub fn new(window: Window) -> anyhow::Result<XRay> {
        let filesystem = Arc::new(Filesystem::new()?);

        let mut app = XRay {
            loaded: false,
            ll_dwReference: 0,
            max_load_stage: 0,
            levels: Vec::new(),
            current_level: None,
            loading_screen: None,
            renderer: select_renderer(window, filesystem.clone())?,
            filesystem,
        };

        app.level_scan();

        Ok(app)
    }

    pub fn level_scan(&mut self) {
        self.levels.clear();
    }
}

fn select_renderer(
    window: Window,
    filesystem: Arc<Filesystem>,
) -> anyhow::Result<Box<dyn Renderer + Send>> {
    let renderer = Box::new(pollster::block_on(WgpuRenderer::new(window, filesystem))?);

    Ok(renderer)
}
