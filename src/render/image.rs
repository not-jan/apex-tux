use std::{
    cell::RefCell,
    fs::File,
    io::Read,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use apex_hardware::FrameBuffer;
use embedded_graphics::{
    image::{Image, ImageRaw},
    pixelcolor::BinaryColor,
    prelude::Point,
    Drawable,
};
use image::{AnimationDecoder, DynamicImage};

static GIF_MISSING: &[u8] = include_bytes!("./../../assets/gif_missing.gif");
static DISPLAY_HEIGHT: i32 = 40;
static DISPLAY_WIDTH: i32 = 128;

pub struct ImageRenderer {
    stop: Point,
    origin: Point,
    decoded_frames: Vec<Vec<u8>>,
    current_frame: AtomicUsize,
    delays: Vec<u16>,
    time_frame_last_update: Rc<RefCell<Instant>>,
}

impl ImageRenderer {
    pub fn calculate_median_color_value(
        image: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
        image_height: i32,
        image_width: i32,
    ) -> u8 {
        //NOTE we're using the median to determine wether the pixel should be black or
        // white

        let mut colors = (0..=255).into_iter().map(|_| 0).collect::<Vec<u32>>();
        let num_pixels = image_width as u32 * image_height as u32;

        let height = image.height();
        let width = image.width();

        for y in 0..image_height {
            //if y is outside of the gif width
            if y >= height as i32 {
                continue;
            }

            //if y is outside of the screen
            if y >= DISPLAY_HEIGHT {
                continue;
            }
            for x in 0..image_width {
                //if x is outside of the gif width
                if x >= width as i32 {
                    continue;
                }

                //if x is outside of the screen
                if x >= DISPLAY_WIDTH {
                    continue;
                }

                let pixel = image.get_pixel(x as u32, y as u32);

                let avg_pixel_value =
                    ((u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])) / 3)
                        as usize;

                //the value is multiplied by the alpha (a) of said pixel
                //the more the pixel is transparent, the less the pixel has an importance
                colors[avg_pixel_value] += u32::from(pixel[3]) / 255;
            }
        }

        let mut sum = 0;
        for (color_value, count) in colors.iter().enumerate() {
            sum += *count;

            if sum >= num_pixels / 2 {
                if color_value == 0 {
                    return 1;
                }
                return color_value as u8;
            }
        }

        1
    }

    pub fn read_image(
        image: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
        image_height: i32,
        image_width: i32,
    ) -> Vec<u8> {
        // We first get the median "color" of the frame
        let median_color = Self::calculate_median_color_value(image, image_height, image_width);

        let mut frame_data = Vec::new();
        let mut buf: u8 = 0;

        let height = image.height();
        let width = image.width();

        for y in 0..image_height {
            //if y is outside of the gif width
            if y >= height as i32 {
                continue;
            }

            //if y is outside of the screen
            if y >= DISPLAY_HEIGHT {
                continue;
            }
            for x in 0..image_width {
                //since we're using an array of u8, every 8 bit we need to start with a new int
                if x % 8 == 0 && x != 0 {
                    frame_data.push(buf);
                    buf = 0;
                }
                //if x is outside of the gif width
                if x >= width as i32 {
                    continue;
                }

                //if x is outside of the screen
                if x >= DISPLAY_WIDTH {
                    continue;
                }

                //getting the value of the pixel
                let pixel = image.get_pixel(x as u32, y as u32);

                let mean = (u32::from(pixel[0]) / 3)
                    + (u32::from(pixel[1]) / 3)
                    + (u32::from(pixel[2]) / 3);
                //I'm not sure if we should do something with the alpha channel of the gif
                //I decided not to, but maybe we should

                if mean >= u32::from(median_color) {
                    //which bit to turn on
                    let shift = x % 8;
                    buf += 128 >> shift;
                }
            }
            //we forcibly push the frame to the buffer after each line
            frame_data.push(buf);
            buf = 0;
        }
        frame_data
    }

    pub fn read_dynamic_image(
        origin: Point,
        stop: Point,
        image: DynamicImage,
        buffer: &[u8],
    ) -> Self {
        //we first get the dimension of the image
        let image_height = stop.y - origin.y;
        let image_width = stop.x - origin.x;

        let mut decoded_frames = Vec::new();
        let mut delays = Vec::new();

        if let Ok(gif) = image::codecs::gif::GifDecoder::new(&buffer[..]) {
            //if the image is a gif
            //NOTE we do not check for the size of each frame!

            //we go through each frame
            for frame in gif.into_frames() {
                //TODO we do not handle if the frame isn't formatted properly!
                if let Ok(frame) = frame {
                    delays.push(Duration::from(frame.delay()).as_millis() as u16);
                    let image = frame.into_buffer();
                    decoded_frames.push(Self::read_image(&image, image_height, image_width));
                }
            }
        } else {
            //if the image is a still image
            decoded_frames.push(Self::read_image(
                &image.into_rgba8(),
                image_height,
                image_width,
            ));
            delays.push(500); // Add a default delay of 500ms for single image
                              // rendering
        }

        Self {
            stop,
            origin,
            decoded_frames,
            current_frame: AtomicUsize::new(0),
            delays,
            time_frame_last_update: Rc::new(RefCell::new(Instant::now())),
        }
    }

    pub fn new(origin: Point, stop: Point, mut file: File) -> Self {
        let mut buffer = Vec::new();
        if let Ok(_) = file.read_to_end(&mut buffer) {
            if let Ok(image) = image::load_from_memory(&buffer) {
                Self::read_dynamic_image(origin, stop, image, &buffer)
            } else {
                log::error!("Failed to decode the image.");
                Self::new_error(origin, stop)
            }
        } else {
            log::error!("Failed to read the image file.");
            Self::new_error(origin, stop)
        }
    }

    pub fn new_error(origin: Point, stop: Point) -> Self {
        Self::new_u8(origin, stop, GIF_MISSING)
    }

    pub fn new_u8(origin: Point, stop: Point, u8_array: &[u8]) -> Self {
        if let Ok(image) = image::load_from_memory(u8_array) {
            Self::read_dynamic_image(origin, stop, image, u8_array)
        } else {
            log::error!("Failed to decode the image.");
            Self::new_error(origin, stop)
        }
    }

    pub fn draw(&self, target: &mut FrameBuffer) -> bool {
        let frame = self.current_frame.load(Ordering::Relaxed);

        //get the data for the specified frame
        let frame_data = &self.decoded_frames[frame];

        //convert the data to an ImageRaw
        let raw_image_frame =
            ImageRaw::<BinaryColor>::new(&frame_data, (self.stop.x - self.origin.x) as u32);

        //draw the ImageRaw on the buffer
        let _ = Image::new(&raw_image_frame, self.origin).draw(target);

        //detect if we should change the frame
        let last_display_time = self.time_frame_last_update.borrow().clone();
        let current_time = Instant::now();
        let elapsed_time = current_time - last_display_time;

        if elapsed_time >= Duration::from_millis(u64::from(self.delays[frame])) {
            //the delays in the image crate isn't in increment of 10ms compared to the gif
            // crate! before we had a *10 because of it

            //update the variable only if we update the frame
            *self.time_frame_last_update.borrow_mut() = current_time;

            //increment the current_frame using atomic operations
            let next_frame = frame + 1;

            let has_gif_ended = next_frame >= self.decoded_frames.len();
            if has_gif_ended {
                //reset to frame 0
                self.current_frame.store(0, Ordering::Relaxed);
            } else {
                self.current_frame.store(next_frame, Ordering::Relaxed);
            }
            return has_gif_ended;
        }
        false
    }
}
