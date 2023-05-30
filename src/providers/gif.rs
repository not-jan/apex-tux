use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper, gif},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use config::Config;
use embedded_graphics::{
    geometry::Point,
};
use futures::Stream;
use linkme::distributed_slice;
use log::info;
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};
use std::fs::File;




#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Gif display source.");

    let gif_path = config.get_str("gif.path").unwrap();
    let gif_file = File::open(&gif_path);

    let gif = match gif_file {
        Ok(file) => gif::Gif::new(Point::new(0, 0), Point::new(128, 40), file),
        Err(err) => {
            log::error!("Failed to open GIF file '{}': {}", gif_path, err);
			
            // Use the `new_error` function to create an error GIF
            gif::Gif::new_error(Point::new(0, 0), Point::new(128, 40))
        }
    };

    Ok(Box::new(Gif {gif}))
}

pub struct Gif {
	gif: gif::Gif
}

impl Gif {
    pub fn render(&self) -> Result<FrameBuffer> {
        let mut buffer = FrameBuffer::new();

		self.gif.draw(&mut buffer);

        Ok(buffer)
    }
}

impl ContentProvider for Gif {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(10)); 
		//the delays in gifs are in increments of 10 ms
		//https://docs.rs/gif/latest/gif/struct.Frame.html#structfield.delay
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Ok(try_stream! {
            loop {
                if let Ok(image) = self.render() {
                    yield image;
                }
                interval.tick().await;
            }
        })
    }

    fn name(&self) -> &'static str {
        "gif"
    }
}
