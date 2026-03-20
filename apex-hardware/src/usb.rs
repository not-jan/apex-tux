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

/// Gen 3 OLED protocol constants
const OLED_SUBCMD: u8 = 0x01;
const CHUNK_SIZE: usize = 80;
const REPORT_SIZE: usize = 641;
const CHUNK_OFFSETS: [u16; 8] = [
    0x0000, 0x0050, 0x00A0, 0x00F0, 0x0140, 0x0190, 0x01E0, 0x0230,
];

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
    // Gen 3 devices
    ApexProTKLWirelessGen3 = 0x1646,
    ApexProTKLWirelessGen3Dongle = 0x1644,
}

/// Gen 3 devices use a different OLED interface and protocol
fn is_gen3(product_id: u16) -> bool {
    matches!(product_id, 0x1646 | 0x1644)
}

/// Returns the correct HID interface number for the OLED on this device.
/// Gen 3 devices use interface 3, older devices use interface 1.
fn oled_interface(product_id: u16) -> i32 {
    if is_gen3(product_id) {
        3
    } else {
        1
    }
}

/// Returns the OLED write command byte for Gen 3 devices.
/// Wired uses 0x0C, wireless dongle uses 0x4C.
fn oled_cmd(product_id: u16) -> u8 {
    if product_id == 0x1644 {
        0x4C
    } else {
        0x0C
    }
}

pub struct USBDevice {
    /// An exclusive handle to the Keyboard.
    handle: HidDevice,
    /// Whether this is a Gen 3 device (uses chunked OLED protocol).
    gen3: bool,
    /// The OLED write command byte for Gen 3 (0x0C wired, 0x4C wireless dongle).
    oled_cmd: u8,
}

impl USBDevice {
    pub fn try_connect() -> Result<Self> {
        let api = HidApi::new()?;

        // Get all supported devices by SteelSeries
        let device = api
            .device_list()
            .find(|device| {
                device.vendor_id() == STEELSERIES_VENDOR_ID
                    && SupportedDevice::try_from(device.product_id()).is_ok()
                    && device.interface_number() == oled_interface(device.product_id())
            })
            .ok_or_else(|| anyhow!("No supported SteelSeries device found!"))?;

        let gen3 = is_gen3(device.product_id());
        let oled_cmd = oled_cmd(device.product_id());

        // This requires udev rules to be setup properly.
        let handle = device.open_device(&api)?;

        Ok(Self {
            handle,
            gen3,
            oled_cmd,
        })
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

    /// Convert the row-major Msb0 framebuffer to SSD1306-style page-major format
    /// and send using the Gen 3 chunked protocol (8 × 641-byte feature reports).
    ///
    /// The FrameBuffer stores pixels as a BitArray<[u8; 642], Msb0> with a 0x61
    /// header byte. Each row is 16 bytes (128 pixels / 8 bits). The Gen 3 OLED
    /// expects SSD1306 page-major format: 5 pages of 128 bytes, each byte holding
    /// 8 vertical pixels (bit 0 = topmost).
    ///
    /// The conversion transposes in 8×8 blocks: each iteration reads 8 source
    /// bytes (one per row in the page, same column-group of 8 pixels) and produces
    /// 8 destination bytes (one per column, each packing 8 vertical pixels).
    fn draw_gen3(&mut self, display: &FrameBuffer) -> Result<()> {
        let raw = display.framebuffer.as_raw_slice();
        let mut fb = [0u8; 640];

        for page in 0..5usize {
            for col_group in 0..16usize {
                // Source byte for row r, column-group g: raw[1 + r*16 + g]
                // The +1 skips the 0x61 header. Stride of 16 = one row of 128/8 bytes.
                let base = 1 + page * 128 + col_group;
                let rows = [
                    raw[base],
                    raw[base + 16],
                    raw[base + 32],
                    raw[base + 48],
                    raw[base + 64],
                    raw[base + 80],
                    raw[base + 96],
                    raw[base + 112],
                ];

                // Transpose: extract bit (7 - bit_pos) from each row byte to build
                // one destination byte with 8 vertical pixels per column.
                let dst = page * 128 + col_group * 8;
                for bit_pos in 0..8usize {
                    let s = 7 - bit_pos;
                    fb[dst + bit_pos] = ((rows[0] >> s) & 1)
                        | (((rows[1] >> s) & 1) << 1)
                        | (((rows[2] >> s) & 1) << 2)
                        | (((rows[3] >> s) & 1) << 3)
                        | (((rows[4] >> s) & 1) << 4)
                        | (((rows[5] >> s) & 1) << 5)
                        | (((rows[6] >> s) & 1) << 6)
                        | (((rows[7] >> s) & 1) << 7);
                }
            }
        }

        for (i, &offset) in CHUNK_OFFSETS.iter().enumerate() {
            let mut report = [0u8; REPORT_SIZE];
            report[0] = self.oled_cmd;
            report[1] = OLED_SUBCMD;
            let offset_bytes = offset.to_le_bytes();
            report[2] = offset_bytes[0];
            report[3] = offset_bytes[1];
            report[4] = CHUNK_SIZE as u8;

            let start = i * CHUNK_SIZE;
            report[6..6 + CHUNK_SIZE].copy_from_slice(&fb[start..start + CHUNK_SIZE]);

            self.handle.send_feature_report(&report)?;
        }

        Ok(())
    }
}

impl Device for USBDevice {
    fn draw(&mut self, display: &FrameBuffer) -> Result<()> {
        if self.gen3 {
            self.draw_gen3(display)
        } else {
            Ok(self
                .handle
                .send_feature_report(display.framebuffer.as_raw_slice())?)
        }
    }

    fn clear(&mut self) -> Result<()> {
        let display = FrameBuffer::new();
        <Self as Device>::draw(self, &display)
    }

    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
