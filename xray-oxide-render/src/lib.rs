use winit::dpi::PhysicalSize;
use winit::window::Window;

pub trait Renderer {
    fn window(&self) -> &Window;
    fn resize(&mut self, new_size: Option<PhysicalSize<u32>>);
    fn render(&mut self) -> anyhow::Result<()>;
}
