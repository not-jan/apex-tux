use anyhow::Result;

pub use apex_hardware::FrameBuffer;
use futures_core::Stream;

pub trait ContentProvider {
    type ContentStream<'a>: Stream<Item = Result<FrameBuffer>> + 'a
    where
        Self: 'a + Sized;

    fn stream(&mut self) -> Result<Self::ContentStream<'_>>
    where
        Self: Sized;
    fn name(&self) -> &'static str;
}
