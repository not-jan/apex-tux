use anyhow::Result;

pub use apex_hardware::FrameBuffer;
use futures_core::Stream;

pub trait ContentProvider {
    type ContentStream<'a>: Stream<Item = Result<FrameBuffer>> + 'a
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>>;
    fn name(&self) -> &'static str;
}
