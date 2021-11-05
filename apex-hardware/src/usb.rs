use crate::{device::FrameBuffer, Device};
use anyhow::{anyhow, Result};
use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyleBuilder, Rectangle, StyledDrawable},
};
use hidapi::{HidApi, HidDevice};
use num_enum::TryFromPrimitive;

/// The SteelSeries vendor ID used to identify the USB devices
pub static STEELSERIES_VENDOR_ID: u16 = 0x1038;

#[repr(u16)]
#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
/// This enum contains the product IDs of currently supported devices
/// If your device is not in this enum it doesn't mean that it won't work, it
/// just means that no one has tried it or bothered to add it yet.
enum SupportedDevice {
    ApexProTKL = 0x1614,
    // Never tested
    Apex7 = 0x1612,
    ApexPro = 0x1610,
    Apex7TKL = 0x1618,
    Apex5 = 0x161C,
}

pub struct USBDevice {
    /// An exclusive handle to the Keyboard.
    handle: HidDevice,
}

impl USBDevice {
    pub fn try_connect() -> Result<Self> {
        let api = HidApi::new()?;

        // Get all supported devices by SteelSeries
        let device = api
            .device_list()
            .find(|device| {
                device.vendor_id() == STEELSERIES_VENDOR_ID &&
                    SupportedDevice::try_from(device.product_id()).is_ok() &&
                    // We only care for the first interface
                    device.interface_number() == 1
            })
            .ok_or_else(|| anyhow!("No supported SteelSeries device found!"))?;

        // This requires udev rules to be setup properly.
        let handle = device.open_device(&api)?;

        Ok(Self { handle })
    }

    pub fn fill(&mut self) -> Result<()> {
        let mut buffer = FrameBuffer::new();
        let style = PrimitiveStyleBuilder::new()
            .fill_color(BinaryColor::On)
            .build();
        Rectangle::new(Point::new(0, 0), buffer.size()).draw_styled(&style, &mut buffer)?;
        self.draw(&buffer)?;
        Ok(())
    }
}

impl Device for USBDevice {
    fn draw(&mut self, display: &FrameBuffer) -> Result<()> {
        Ok(self
            .handle
            .send_feature_report(display.framebuffer.as_buffer())?)
    }

    fn clear(&mut self) -> Result<()> {
        let display = FrameBuffer::new();
        <Self as Device>::draw(self, &display)
    }
}
