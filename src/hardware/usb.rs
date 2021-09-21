use crate::{
    hardware::device::Device,
    render::{display::FrameBuffer, scheduler},
};
use anyhow::{anyhow, Result};
use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget};
use hidapi::{HidApi, HidDevice};
use num_enum::TryFromPrimitive;
use std::convert::TryFrom;
use tauri_hotkey::{Hotkey, HotkeyManager, Key, Modifier};
use tokio::sync::mpsc;

static STEELSERIES_VENDOR_ID: u16 = 0x1038;

#[repr(u16)]
#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
enum SupportedDevice {
    ApexPro = 0x1614,
    // Never tested
    Apex7 = 0x1612,
}

pub struct USBDevice {
    /// An exclusive handle to the Keyboard.
    handle: HidDevice,
    /// The hot key manager that handles the key combinations to change the
    /// screen It'll unregister all keys if dropped so it has to be
    /// persisted even if unused.
    _hkm: HotkeyManager,
}

impl USBDevice {
    pub fn try_connect(sender: mpsc::Sender<scheduler::Command>) -> Result<Self> {
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

        // This required udev rules to be setup properly.
        let handle = device.open_device(&api)?;

        let mut hkm = HotkeyManager::new();

        let modifiers = vec![Modifier::ALT, Modifier::SHIFT];

        let sender2 = sender.clone();

        hkm.register(
            Hotkey {
                modifiers: modifiers.clone(),
                keys: vec![Key::A],
            },
            move || {
                sender
                    .blocking_send(scheduler::Command::PreviousSource)
                    .expect("Failed to send command!");
            },
        )?;
        hkm.register(
            Hotkey {
                modifiers,
                keys: vec![Key::D],
            },
            move || {
                sender2
                    .blocking_send(scheduler::Command::NextSource)
                    .expect("Failed to send command!");
            },
        )?;

        Ok(USBDevice { handle, _hkm: hkm })
    }
}

impl Device for USBDevice {
    fn draw(&mut self, display: &FrameBuffer) -> Result<()> {
        Ok(self
            .handle
            .send_feature_report(display.framebuffer.as_buffer())?)
    }

    fn clear(&mut self) -> Result<()> {
        let mut display = FrameBuffer::new();
        display.clear(BinaryColor::Off)?;
        self.draw(&display)
    }
}
