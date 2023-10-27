use byteorder::LittleEndian;
use std::io::Cursor;
use std::num::NonZeroU32;

use image::io::Reader as ImageReader;
use image::{ImageFormat, RgbImage};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
use winit::window::{Window, WindowBuilder, WindowId, WindowLevel};

use crate::engine::ext::WindowExt;

const WIDTH: u32 = 500;
const HEIGHT: u32 = 268;
const IMAGE_DATA: &[u8] = include_bytes!("splash.bmp");

struct SplashScreen {
    window: Option<Window>,
    window_id: Option<WindowId>,
    splash_image: RgbImage,
}

impl SplashScreen {
    fn draw_to_window(&self, window: &Window) {
        let context = unsafe { softbuffer::Context::new(window) }.unwrap();
        let mut surface = unsafe { softbuffer::Surface::new(&context, window) }.unwrap();
        let width = unsafe { NonZeroU32::new_unchecked(WIDTH) };
        let height = unsafe { NonZeroU32::new_unchecked(HEIGHT) };

        surface.resize(width, height).unwrap();

        let mut buffer = surface.buffer_mut().unwrap();

        self.splash_image
            .enumerate_pixels()
            .for_each(|(x, y, pixel)| {
                let [red, green, blue] = pixel.0;
                let bytes = [0, red, green, blue];
                let pixel = u32::from_be_bytes(bytes);

                buffer[(y * WIDTH + x) as usize] = pixel;
            });

        buffer.present().unwrap();
    }
}

pub fn show_splash(event_loop: &mut EventLoop<()>) -> anyhow::Result<()> {
    let splash_image = ImageReader::with_format(Cursor::new(IMAGE_DATA), ImageFormat::Bmp)
        .decode()?
        .to_rgb8();

    let mut splash_screen = SplashScreen {
        window_id: None,
        window: None,
        splash_image,
    };

    event_loop.run_on_demand(move |event, target| {
        target.set_control_flow(ControlFlow::Wait);

        if let Event::UserEvent(_) = event {
            splash_screen.window = None;
        } else if let Some(window) = &splash_screen.window {
            if let Event::WindowEvent { window_id, event } = event {
                if window_id == window.id() {
                    match event {
                        WindowEvent::RedrawRequested => {
                            splash_screen.draw_to_window(window);
                        }
                        WindowEvent::CloseRequested => {
                            splash_screen.window = None;
                        }
                        _ => {}
                    }
                }
            }
        } else if let Some(id) = splash_screen.window_id {
            if let Event::WindowEvent {
                window_id,
                event: WindowEvent::Destroyed,
            } = event
            {
                if window_id == id {
                    splash_screen.window_id = None;
                    target.exit();
                }
            }
        } else if let Event::Resumed = event {
            let window = WindowBuilder::new()
                .with_title("XRay Oxide")
                .with_inner_size(LogicalSize::new(WIDTH as f64, HEIGHT as f64))
                .with_resizable(false)
                .with_decorations(false)
                .with_visible(false)
                .build(target)
                .unwrap();

            window.center_window(target);

            window.set_visible(true);
            window.set_window_level(WindowLevel::AlwaysOnTop);

            splash_screen.window_id = Some(window.id());
            splash_screen.window = Some(window);
        }
    })?;

    Ok(())
}
