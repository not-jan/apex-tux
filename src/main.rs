#![allow(incomplete_features)]
#![feature(
    type_alias_impl_trait,
    try_blocks,
    inherent_associated_types,
    async_iterator,
    decl_macro,
    impl_trait_in_assoc_type
)]
#![warn(clippy::pedantic)]
// `clippy::mut_mut` is disabled because `futures::stream::select!` causes the lint to fire
// The other lints are just awfully tedious to implement especially when dealing with pixel
// coordinates. I'll fix them if I'm ever that bored.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    missing_copy_implementations,
    unused_qualifications
)]

extern crate embedded_graphics;

use anyhow::Result;
use log::warn;

// This is kind of pointless on non-Linux platforms
#[cfg(all(feature = "dbus-support", target_os = "linux"))]
mod dbus;

mod providers;
mod render;

#[cfg(all(feature = "simulator", feature = "usb"))]
compile_error!(
    "The features `simulator` and `usb` are mutually exclusive. Use --no-default-features!"
);

#[cfg(feature = "simulator")]
use apex_simulator::Simulator;

use crate::render::{scheduler, scheduler::Scheduler};
#[cfg(feature = "engine")]
use apex_engine::Engine;
use apex_hardware::AsyncDevice;
#[cfg(all(feature = "usb", target_os = "linux", not(feature = "engine")))]
use apex_hardware::USBDevice;
use log::{info, LevelFilter};
use simplelog::{Config as LoggerConfig, SimpleLogger};
use tokio::sync::broadcast;

use apex_input::Command;

#[tokio::main]
#[allow(clippy::missing_errors_doc)]
#[allow(clippy::missing_panics_doc)]
pub async fn main() -> Result<()> {
    SimpleLogger::init(LevelFilter::Info, LoggerConfig::default())?;

    // This channel is used to send commands to the scheduler
    let (tx, rx) = broadcast::channel::<Command>(100);
    #[cfg(all(feature = "usb", target_family = "unix", not(feature = "engine")))]
    let mut device = USBDevice::try_connect()?;

    #[cfg(feature = "hotkeys")]
    let hkm = apex_input::InputManager::new(tx.clone());

    #[cfg(feature = "engine")]
    let mut device = Engine::new().await?;

    let mut settings = config::Config::default();
    // Add in `$USER_CONFIG_DIR/apex-tux/settings.toml`
    if let Some(user_config_dir) = dirs::config_dir() {
        settings.merge(
            config::File::with_name(&user_config_dir.join("apex-tux/settings").to_string_lossy())
                .required(false),
        )?;
    }
    settings
        // Add in `./settings.toml`
        .merge(config::File::with_name("settings").required(false))?
        // Add in settings from the environment (with a prefix of APEX)
        // Eg.. `APEX_DEBUG=1 ./target/app` would set the `debug` key
        .merge(config::Environment::with_prefix("APEX_"))?;

    #[cfg(feature = "simulator")]
    let mut device = Simulator::connect(tx.clone());

    device.clear().await?;

    let mut scheduler = Scheduler::new(device);
    scheduler.start(tx.clone(), rx, settings).await?;

    ctrlc::set_handler(move || {
        info!("Ctrl + C received, shutting down!");
        tx.send(Command::Shutdown)
            .expect("Failed to send shutdown signal!");
    })?;

    #[cfg(feature = "hotkeys")]
    drop(hkm);

    Ok(())
}
