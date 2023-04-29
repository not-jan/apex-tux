use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use chrono::{DateTime, Local};
use config::Config;
use embedded_graphics::{
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::BinaryColor,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
	image::{Image,ImageRaw},
};
use futures::Stream;
use gif::{Decoder, Frame};
use image::ImageBuffer;
use linkme::distributed_slice;
use log::info;
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};
use std::fs::File;
use std::sync::atomic::{AtomicUsize, Ordering};

static ACTUAL_FRAME: AtomicUsize = AtomicUsize::new(0);

static DISPLAY_HEIGHT: u16 = 40;
static DISPLAY_WIDTH: u16 = 128;

#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

fn calculate_median_color_value(frame: &Frame) -> u8 {
    let mut colors = (0..=255).into_iter().map(|_| 0).collect::<Vec<u32>>();
    let num_pixels = frame.width as u32 * frame.height as u32;

	let mut buf_r:u8 = 0;
	let mut buf_g:u8 = 0; 
	let mut buf_b:u8 = 0; 
    for (i, byte) in frame.buffer.iter().enumerate() {
		if i % 4 == 0 && i != 0 {
			buf_r = 0;
			buf_g = 0;
			buf_b = 0;
		}
		if i % 4 == 3{
			colors[(buf_r/3 + buf_g/3 + buf_b/3) as usize] += 1;
		}
		if i %4 == 0 { 
			buf_r  = *byte;
		}
		if i %4 == 1 { 
			buf_g  = *byte;
		}
		if i %4 == 2 { 
			buf_b  = *byte;
		}
        
    }

    let mut sum = 0;
    for (color_value, count) in colors.iter().enumerate() {
        sum += *count;

        if sum >= num_pixels / 2 {
			if color_value == 0{
				return 1;
			}
            return color_value as u8;
        }
    }

    1
}

fn convert_vec_to_array<T, const N: usize>(v: Vec<T>) -> [T; N] {
    v.try_into()
        .unwrap_or_else(|v: Vec<T>| panic!("Expected a Vec of length {} but it was {}", N, v.len()))
}
#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Gif display source.");

    let gif_file = File::open(config.get_str("gif.path").unwrap_or("gifs/sample_1.gif".to_string())).unwrap();

	let mut decoder = gif::DecodeOptions::new();

	decoder.set_color_output(gif::ColorOutput::RGBA);

	let mut decoder = decoder.read_info(gif_file).unwrap();
	let mut decoded_frames = Vec::new();

    // Read all the frames in the GIF file.

	while let Some(frame) = decoder.read_next_frame().unwrap() {
		
		let median_color = calculate_median_color_value(frame);

		let mut image = Vec::new();
		let mut buf: u8 = 0;
		let width= u64::from(frame.width);

		let pixels = &frame.buffer;
		for y in 0..DISPLAY_HEIGHT{
			for x in 0..DISPLAY_WIDTH{
				if x % 8 == 0  && x != 0{
					image.push(buf);
					buf = 0;
				} 
				if x as u64 >= width{
					continue;
				}
				let start:u64 = ((y as u64) * width + (x as u64))*4;
				
				let pixel_r = pixels.get(start as usize).unwrap_or(&0);
				let pixel_g = pixels.get((start+1) as usize).unwrap_or(&0);
				let pixel_b = pixels.get((start+2) as usize).unwrap_or(&0);

				let mean = pixel_r/3 + pixel_g/3 + pixel_b/3;

				if mean >= median_color{
					let shift = x%8;
					buf = buf + ( 128 >> shift ) ;
				}

			}
			image.push(buf);
			buf = 0;
		}
        decoded_frames.push(convert_vec_to_array(image));
    }

    Ok(Box::new(Gif { decoded_frames}))
}

pub struct Gif {
	decoded_frames : Vec<[u8; 128*40/8]>
}

impl Gif {
    pub fn render(&self) -> Result<FrameBuffer> {
		let mut frame = ACTUAL_FRAME.load(Ordering::SeqCst);
		ACTUAL_FRAME.fetch_add(1, Ordering::SeqCst);
		if frame == self.decoded_frames.len(){
			ACTUAL_FRAME.store(0, Ordering::SeqCst);
			frame = 0;
		}

        // Create a black buffer with the same size as the display
        let mut buffer = FrameBuffer::new();

        // Loop over each frame in the gif
        let frame_data = self.decoded_frames[frame];


		let raw_image = ImageRaw::<BinaryColor>::new(&frame_data, 128);
		// Create an Image object from the frame data
		let frame_image = Image::new(
			&raw_image,
			Point::new(0, 0)
		);

		// Draw the image onto the buffer
		frame_image.draw(&mut buffer)?;
        

        Ok(buffer)
    }
}

impl ContentProvider for Gif {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(100));
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