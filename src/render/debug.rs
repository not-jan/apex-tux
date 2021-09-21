use crate::render::{
    display::{ContentProvider, FrameBuffer},
    scheduler::{ContentWrapper, CONTENT_PROVIDERS},
};
use anyhow::Result;
use async_stream::try_stream;
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::Point,
    primitives::{Line, Primitive, PrimitiveStyle},
    Drawable,
};
use futures::Stream;
use linkme::distributed_slice;
use log::info;
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};

#[distributed_slice(CONTENT_PROVIDERS)]
static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[allow(clippy::unnecessary_wraps)]
fn register_callback() -> Result<Box<dyn ContentWrapper>> {
    info!("Registering dummy display source.");
    let provider = Box::new(DummyProvider {});
    Ok(provider)
}

struct DummyProvider;

impl ContentProvider for DummyProvider {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(50));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Ok(try_stream! {
            let mut x_index = 0;
            let mut y_index = 0;

            let style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);

            loop {
                let mut display = FrameBuffer::new();
                Line::new(Point::new(x_index, 0), Point::new(x_index, 39)).into_styled(style).draw(&mut display)?;
                Line::new(Point::new(0, y_index), Point::new(127, y_index)).into_styled(style).draw(&mut display)?;
                yield display;
                interval.tick().await;
                x_index = x_index.wrapping_add(1) % 128;
                y_index = y_index.wrapping_add(1) % 40;
            }
        })
    }

    fn name(&self) -> &'static str {
        "dummy"
    }
}
