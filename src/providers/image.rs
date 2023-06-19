use crate::{
    render::{display::ContentProvider, image, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use config::Config;
use embedded_graphics::geometry::Point;
use futures::Stream;
use linkme::distributed_slice;
use log::info;
use std::{
    fs::{self, File},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};

#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Image display source.");

    let image_path = config
        .get_str("image.path")
        .unwrap_or_else(|_| String::from("images/sample_1.gif"));
    let mut images = Vec::new();

    if Path::new(&image_path).is_dir() {
        for file in fs::read_dir(image_path).unwrap() {
            let file_path = file.unwrap().path();
            let image_file = File::open(file_path.clone());

            let image = match image_file {
                Ok(file) => image::ImageRenderer::new(Point::new(0, 0), Point::new(128, 40), file),
                Err(err) => {
                    log::error!(
                        "Failed to open the image '{}': {}",
                        file_path.display(),
                        err
                    );

                    // Use the `new_error` function to create an error GIF
                    image::ImageRenderer::new_error(Point::new(0, 0), Point::new(128, 40))
                }
            };
            images.push(image);
        }
    } else {
        let image_file = File::open(&image_path);

        let image = match image_file {
            Ok(file) => image::ImageRenderer::new(Point::new(0, 0), Point::new(128, 40), file),
            Err(err) => {
                log::error!("Failed to open the image '{}': {}", image_path, err);

                // Use the `new_error` function to create an error GIF
                image::ImageRenderer::new_error(Point::new(0, 0), Point::new(128, 40))
            }
        };
        images.push(image);
    }

    Ok(Box::new(Images {
        images,
        current_image: AtomicUsize::new(0),
    }))
}

pub struct Images {
    images: Vec<image::ImageRenderer>,
    current_image: AtomicUsize,
}

impl Images {
    pub fn render(&self) -> Result<FrameBuffer> {
        let mut buffer = FrameBuffer::new();

        let image = self.current_image.load(Ordering::Relaxed);
        //get the data for the specified frame
        let image_data = &self.images[image];
        //draw and detect if the gif/image has ended
        let has_ended = image_data.draw(&mut buffer);

        if has_ended {
            let mut next_image = image + 1;
            let has_no_next_image = next_image >= self.images.len();

            if has_no_next_image {
                //reset to frame 0
                next_image = 0;
            } else {
                next_image = next_image;
            }

            self.current_image.store(next_image, Ordering::Relaxed);
            let next = &self.images[next_image];
            next.set_display_time();
        }

        Ok(buffer)
    }
}

impl ContentProvider for Images {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(10));
        //the delays in gifs are in increments of 10 ms
        // from wikipedia (in the table, look for the byte 324)
        // https://en.wikipedia.org/w/index.php?title=GIF&oldid=1157626024#Animated_GIF
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
        "image"
    }
}
