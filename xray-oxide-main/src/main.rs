#[cfg(desktop)]
fn main() -> anyhow::Result<()> {
    xray_oxide_engine::entry_point()
}

#[cfg(not(desktop))]
fn main() {
    panic!("This application is not supported on this platform");
}
