pub trait Renderer {

}

pub struct GlRenderer {

}

impl GlRenderer {
    pub fn new() -> anyhow::Result<GlRenderer> {
        Ok(GlRenderer {})
    }
}

impl Renderer for GlRenderer {}