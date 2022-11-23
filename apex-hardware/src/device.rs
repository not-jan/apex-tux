use anyhow::Result;
use bitvec::prelude::*;
use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
#[cfg(feature = "async")]
use std::future::Future;

const FB_SIZE: usize = 40 * 128 / 8 + 2;

#[derive(Copy, Clone, Debug)]
pub struct FrameBuffer {
    /// The framebuffer with one bit value per pixel.
    /// Two extra bytes are added, one for the header byte `0x61` and one for a
    /// trailing null byte. This is done to prevent superfluous copies when
    /// sending the image to a display device. The implementations of
    /// `Drawable` and `DrawTarget` take this quirk into account.
    pub framebuffer: BitArray<[u8; FB_SIZE], Msb0>,
}

impl Default for FrameBuffer {
    fn default() -> Self {
        let mut framebuffer = BitArray::<[u8; FB_SIZE], Msb0>::ZERO;
        framebuffer.as_raw_mut_slice()[0] = 0x61;
        FrameBuffer { framebuffer }
    }
}

impl FrameBuffer {
    /// Initializes a new `FrameBuffer` with all pixels set to
    /// `BinaryColor::Off `
    pub fn new() -> Self {
        Self::default()
    }
}

/// This trait represents a device that can receive new images to be displayed.
pub trait Device {
    /// Sends a `FrameBuffer` to the device.
    fn draw(&mut self, display: &FrameBuffer) -> Result<()>;
    /// Convenience method for clearing the whole screen.
    /// Most implementations will send an empty `FrameBuffer` to `Device::draw`
    /// but there may be more efficient ways for some devices to implement here.
    fn clear(&mut self) -> Result<()>;

    fn shutdown(&mut self) -> Result<()>;
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

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        Size::new(128, 40)
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
                self.framebuffer.set(index as u32 as usize, color.is_on());
            }
        }

        Ok(())
    }
}

#[cfg(feature = "async")]
pub trait AsyncDevice {
    type DrawResult<'a>: Future<Output = Result<()>> + 'a
    where
        Self: 'a;
    type ClearResult<'a>: Future<Output = Result<()>> + 'a
    where
        Self: 'a;

    type ShutdownResult<'a>: Future<Output = Result<()>> + 'a
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn draw<'this>(&'this mut self, display: &'this FrameBuffer) -> Self::DrawResult<'this>;
    #[allow(clippy::needless_lifetimes)]
    fn clear<'this>(&'this mut self) -> Self::ClearResult<'this>;
    #[allow(clippy::needless_lifetimes)]
    fn shutdown<'this>(&'this mut self) -> Self::ShutdownResult<'this>;
}

#[cfg(feature = "async")]
impl<T: Device> AsyncDevice for T
where
    T: 'static,
{
    type ClearResult<'a> = impl Future<Output = Result<()>> + 'a
    where
        Self: 'a;
    type DrawResult<'a> = impl Future<Output = Result<()>> + 'a
    where
        Self: 'a;
    type ShutdownResult<'a> = impl Future<Output = Result<()>> + 'a
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn draw<'this>(&'this mut self, display: &'this FrameBuffer) -> Self::DrawResult<'this> {
        let x = <Self as Device>::draw(self, display);
        async { x }
    }

    #[allow(clippy::needless_lifetimes)]
    fn clear<'this>(&'this mut self) -> Self::ClearResult<'this> {
        let x = <Self as Device>::clear(self);
        async { x }
    }

    #[allow(clippy::needless_lifetimes)]
    fn shutdown<'this>(&'this mut self) -> Self::ShutdownResult<'this> {
        let x = <Self as Device>::shutdown(self);
        async { x }
    }
}
