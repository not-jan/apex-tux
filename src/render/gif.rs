use std::{sync::atomic::{AtomicUsize, Ordering}, fs::File};

use apex_hardware::FrameBuffer;

use embedded_graphics::{prelude::{Point}, image::{ImageRaw, Image}, pixelcolor::BinaryColor, Drawable};
use gif::{Frame};
use log::warn;

//TODO the gif itself is way too fast (made it dirtly in photoshop)
static GIF_MISSING: &[u8] = include_bytes!("./../../assets/gif_missing.gif");


static DISPLAY_HEIGHT: i32 = 40;
static DISPLAY_WIDTH: i32 = 128;

pub struct Gif{
    stop: Point,
    origin: Point,
    decoded_frames: Vec<Vec<u8>>,
    current_frame: AtomicUsize,
}


impl Gif{

	pub fn calculate_median_color_value(frame: &Frame, gif_height: i32, gif_width: i32) -> u8 {
		//NOTE we're using the median to determine wether the pixel should be black or white 

		let mut colors = (0..=255).into_iter().map(|_| 0).collect::<Vec<u32>>();
	
		//the u64 is just in case someone put a gif that's huge (in terms of resolution), it shouldn't break
		let width= u64::from(frame.width);
		let height= u64::from(frame.height);
		
		let num_pixels = gif_width as u32 * gif_height as u32;

		let pixels = &frame.buffer;

		for y in 0..gif_height{
			//if x is outside of the gif width
			if y as u64 >= height{
				continue;
			}

			//if x is outside of the screen
			if y >= DISPLAY_HEIGHT{
				continue;
			}
			for x in 0..gif_width{
				//if x is outside of the gif width
				if x as u64 >= width{
					continue;
				}

				//if x is outside of the screen
				if x >= DISPLAY_WIDTH{
					continue;
				}

				//calculating the index
				let start:u64 = ((y as u64) * width + (x as u64))*4;

				//getting the value of the pixels
				let pixel_r = pixels.get(start as usize).unwrap_or(&0);
				let pixel_g = pixels.get((start+1) as usize).unwrap_or(&0);
				let pixel_b = pixels.get((start+2) as usize).unwrap_or(&0);
				let pixel_a = pixels.get((start+3) as usize).unwrap_or(&0);

				//the value is multiplied by the alpha of said pixel
				//the more the pixel is transparent, the less the pixel has an importance
				colors [(pixel_r/3 + pixel_g/3 + pixel_b/3) as usize] += u32::from(pixel_a / 255);
			}
		}
	
		let mut sum = 0;
		for (color_value, count) in colors.iter().enumerate() {
			sum += *count;
	
			if u32::from(sum) >= num_pixels / 2 {
				if color_value == 0{
					return 1;
				}
				return color_value as u8;
			}
		}
	
		1
	}

	pub fn read_frame(frame: &Frame, gif_height: i32, gif_width: i32) -> Vec<u8>{

		let median_color = Self::calculate_median_color_value(frame, gif_height, gif_width);

		let mut image = Vec::new();
		let mut buf: u8 = 0;

		//the u64 is just in case someone put a gif that's huge (in terms of resolution), it shouldn't break
		let width= u64::from(frame.width);
		let height= u64::from(frame.height);

		let pixels = &frame.buffer;

		for y in 0..gif_height{
			//if x is outside of the gif width
			if y as u64 >= height{
				continue;
			}

			//if x is outside of the screen
			if y >= DISPLAY_HEIGHT{
				continue;
			}
			for x in 0..gif_width{
				//since we're using an array of u8, every 8 bit we need to start with a new int
				if x % 8 == 0  && x != 0{
					image.push(buf);
					buf = 0;
				} 
				//if x is outside of the gif width
				if x as u64 >= width{
					continue;
				}

				//if x is outside of the screen
				if x >= DISPLAY_WIDTH{
					continue;
				}

				//calculating the index
				let start:u64 = ((y as u64) * width + (x as u64))*4;

				//getting the value of the pixels
				let pixel_r = pixels.get(start as usize).unwrap_or(&0);
				let pixel_g = pixels.get((start+1) as usize).unwrap_or(&0);
				let pixel_b = pixels.get((start+2) as usize).unwrap_or(&0);

				let mean = pixel_r/3 + pixel_g/3 + pixel_b/3;

				if mean >= median_color{
					//which bit to turn on
					let shift = x%8;
					buf = buf + ( 128 >> shift ) ;
				}

			}
			//we fortcibly push the frame to the buffer after each line
			image.push(buf);
			buf = 0;
		}
		return image
	}

    pub fn new(origin: Point, stop: Point, file: File) -> Self {
		let gif_height = stop.y - origin.y;
		let gif_width = stop.x - origin.x;
		

		let mut decoder = gif::DecodeOptions::new();

		decoder.set_color_output(gif::ColorOutput::RGBA);


		let decoder_result= std::panic::catch_unwind(||decoder.read_info(file).unwrap());

		let mut decoded_frames = Vec::new();
		//this is to handle juste in case the file isn't a gif, or can't be parsed correctly
		match decoder_result {
			Ok(_)=> {
				let mut decoder = decoder_result.unwrap();
		
				// Read all the frames in the GIF 
				while let Some(frame) = decoder.read_next_frame().unwrap() {
					decoded_frames.push(Self::read_frame(frame, gif_height, gif_width));
				}
				Self {
					stop,
					origin,
					decoded_frames,
					current_frame: AtomicUsize::new(0)
				}
			},
			Err(_)=> {
				warn!("The gif file can't be used, using the default placeholder.");

				Self::new_error(origin, stop)
			}
		}
		
    }

	pub fn new_error(origin: Point, stop: Point) -> Self {
		Self::new_u8(origin, stop, GIF_MISSING)
	}

    pub fn new_u8(origin: Point, stop: Point, u8_array: &[u8]) -> Self {
		let gif_height = stop.y - origin.y;
		let gif_width = stop.x - origin.x;
		

		let mut decoder = gif::DecodeOptions::new();

		decoder.set_color_output(gif::ColorOutput::RGBA); //TODO we're repeating a those lines, maybe make a function (don't ask me how) 

		let mut decoder = decoder.read_info(u8_array).unwrap();
		let mut decoded_frames = Vec::new();


		// Read all the frames in the u8 array.
		while let Some(frame) = decoder.read_next_frame().unwrap() {
			decoded_frames.push(Self::read_frame(frame, gif_height, gif_width));
			
		}
        Self {
            stop,
            origin,
            decoded_frames,
			current_frame: AtomicUsize::new(0)
        }
    }



    pub fn draw(
        &self,
        target: &mut FrameBuffer,
    ) -> bool {
        let frame = self.current_frame.load(Ordering::Relaxed);

        //increment the current_frame using atomic operations
        let next_frame = frame + 1;

        let has_gif_ended = next_frame >= self.decoded_frames.len();
        if has_gif_ended {
            //reset to frame 0
            self.current_frame.store(0, Ordering::Relaxed);
        } else {
            self.current_frame.store(next_frame, Ordering::Relaxed);
        }

        //get the data for the specified frame
        let frame_data = &self.decoded_frames[frame];
		
		//convert the data to an ImageRaw
		let raw_gif_frame = ImageRaw::<BinaryColor>::new(&frame_data, 128);
		
		//draw the ImageRaw on the buffer
		let _ = Image::new(
			&raw_gif_frame,
			Point::new(0, 0)
		).draw(target);

        has_gif_ended
    }
}
