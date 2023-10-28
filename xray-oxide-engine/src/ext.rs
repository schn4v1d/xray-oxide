use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event_loop::EventLoopWindowTarget,
    window::Window,
};

pub trait WindowExt {
    fn center_window<T>(&self, target: &EventLoopWindowTarget<T>) {
        let monitor = target
            .primary_monitor()
            .unwrap_or_else(|| target.available_monitors().next().unwrap());

        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let window_size = self.outer_size();

        let cx =
            monitor_position.x + (monitor_size.width / 2) as i32 - (window_size.width / 2) as i32;
        let cy =
            monitor_position.y + (monitor_size.height / 2) as i32 - (window_size.height / 2) as i32;

        self.set_outer_position(PhysicalPosition::new(cx, cy));
    }

    fn outer_size(&self) -> PhysicalSize<u32>;
    fn set_outer_position(&self, position: PhysicalPosition<i32>);
}

impl WindowExt for Window {
    fn outer_size(&self) -> PhysicalSize<u32> {
        self.outer_size()
    }

    fn set_outer_position(&self, position: PhysicalPosition<i32>) {
        self.set_outer_position(position);
    }
}
