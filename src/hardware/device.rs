use crate::render::display::FrameBuffer;
use anyhow::Result;

pub trait Device {
    fn draw(&mut self, display: &FrameBuffer) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
}
