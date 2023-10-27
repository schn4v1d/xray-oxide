#[cfg(desktop)]
pub mod core;
#[cfg(desktop)]
pub mod engine;
#[cfg(desktop)]
pub mod render;

#[cfg(desktop)]
fn main() -> anyhow::Result<()> {
    engine::entry_point()
}

#[cfg(not(desktop))]
fn main() {
    panic!("This application is not supported on this platform");
}
