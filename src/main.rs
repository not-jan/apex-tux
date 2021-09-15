#![allow(incomplete_features)]
#![feature(
    generic_associated_types,
    type_alias_impl_trait,
    const_fn_trait_bound,
    format_args_capture,
    once_cell,
    try_blocks,
    const_fn_floating_point_arithmetic,
    const_generics,
    const_evaluatable_checked,
    inherent_associated_types,
    const_generics_defaults,
    box_into_pin,
    async_closure
)]
#![feature(async_stream)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::mut_mut
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    missing_copy_implementations,
    unused_qualifications
)]

use anyhow::Result;

#[cfg(feature = "dbus-support")]
mod dbus;

mod hardware;
mod providers;
mod render;

#[cfg(feature = "simulator")]
use crate::hardware::simulator::Simulator;
#[cfg(not(feature = "simulator"))]
use crate::hardware::usb::USBDevice;
use crate::{
    hardware::device::Device,
    render::{scheduler, scheduler::Scheduler},
};
use log::{info, LevelFilter};
use simplelog::{Config, SimpleLogger};
#[cfg(not(feature = "simulator"))]
use tauri_hotkey::{Hotkey, HotkeyManager, Key, Modifier};
use tokio::sync::mpsc;

#[tokio::main]
#[allow(clippy::missing_errors_doc)]
pub async fn main() -> Result<()> {
    SimpleLogger::init(LevelFilter::Info, Config::default())?;

    let (tx, rx) = mpsc::channel::<scheduler::Command>(10);

    #[cfg(not(feature = "simulator"))]
    let mut device = USBDevice::try_connect(tx.clone())?;

    #[cfg(feature = "simulator")]
    let mut device = Simulator::connect(tx.clone());

    device.clear()?;

    let mut scheduler = Scheduler::new(device);

    ctrlc::set_handler(move || {
        info!("Ctrl + C received, shutting down!");
        tx.blocking_send(scheduler::Command::Shutdown)
            .expect("Failed to send shutdown signal!");
    })?;

    scheduler.start(rx).await?;

    Ok(())
}
