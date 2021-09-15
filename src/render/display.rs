use anyhow::Result;
use bitvec::prelude::*;
use embedded_graphics::{
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::{DrawTarget, OriginDimensions, Size},
    Drawable, Pixel,
};
use futures_core::Stream;
use num_traits::AsPrimitive;

#[derive(Copy, Clone, Debug)]
pub struct FrameBuffer {
    /// The framebuffer with one bit value per pixel.
    pub(crate) framebuffer: BitArray<Msb0, [u8; 40 * 128 / 8 + 2]>,
}

pub trait ContentProvider {
    type ContentStream<'a>: Stream<Item = Result<FrameBuffer>> + 'a;

    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>>;

    fn name(&self) -> &'static str;
}

impl FrameBuffer {
    pub fn new() -> Self {
        let mut framebuffer = BitArray::<Msb0, [u8; 642]>::zeroed();
        framebuffer.as_mut_buffer()[0] = 0x61;
        FrameBuffer { framebuffer }
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(128, 40)
    }
}

impl Drawable for FrameBuffer {
    type Color = BinaryColor;
    type Output = ();

    fn draw<D>(&self, target: &mut D) -> Result<Self::Output, <D as DrawTarget>::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        let iter = (0..5120).map(|i| {
            let pos = Point::new(i % 128, i / 128);

            Pixel(
                pos,
                if *self.framebuffer.get(i as usize + 8_usize).unwrap() {
                    BinaryColor::On
                } else {
                    BinaryColor::Off
                },
            )
        });

        target.draw_iter(iter)?;

        Ok::<Self::Output, <D as DrawTarget>::Error>(())
    }
}

impl DrawTarget for FrameBuffer {
    type Color = BinaryColor;
    type Error = anyhow::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if let (x @ 0..=127, y @ 0..=39) = (coord.x, coord.y) {
                // Calculate the index in the framebuffer.
                let index: i32 = x + y * 128 + 8;
                self.framebuffer.set(index.as_(), color.is_on());
            }
        }

        Ok(())
    }
}
