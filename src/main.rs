#![allow(incomplete_features)]
#![feature(
    generic_associated_types,
    type_alias_impl_trait,
    const_fn_trait_bound,
    format_args_capture,
    once_cell,
    try_blocks,
    const_fn_floating_point_arithmetic,
    inherent_associated_types,
    const_generics_defaults,
    box_into_pin,
    async_closure,
    async_stream,
    decl_macro
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
use futures::pin_mut;
use log::warn;
use std::{sync::Arc, time::Duration};

#[cfg(feature = "dbus-support")]
mod dbus;

mod hardware;
mod providers;
mod render;

#[cfg(all(feature = "simulator", feature = "usb"))]
compile_error!(
    "The features `simulator` and `usb` are mutually exclusive. Use --no-default-features!"
);

#[cfg(feature = "simulator")]
use apex_simulator::Simulator;

use crate::render::{scheduler, scheduler::Scheduler};
#[cfg(all(feature = "engine"))]
use apex_engine::Engine;
use apex_hardware::AsyncDevice;
#[cfg(feature = "usb")]
use apex_hardware::USBDevice;
use log::{info, LevelFilter};
use simplelog::{Config as LoggerConfig, SimpleLogger};
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinHandle,
    time::MissedTickBehavior,
};

use apex_input::Command;

#[tokio::main]
#[allow(clippy::missing_errors_doc)]
pub async fn main() -> Result<()> {
    SimpleLogger::init(LevelFilter::Info, LoggerConfig::default())?;

    // This channel is used to send commands to the scheduler
    let (tx, mut rx) = broadcast::channel::<Command>(100);
    let mut rx2 = tx.subscribe();
    #[cfg(all(feature = "usb", target_family = "unix", not(feature = "engine")))]
    let mut device = USBDevice::try_connect()?;

    #[cfg(all(feature = "usb"))]
    let hkm = apex_input::InputManager::new(tx.clone());

    #[cfg(all(feature = "engine"))]
    let mut device = Engine::new().await?;
    let ctl = device.clone();

    let handle: JoinHandle<Result<()>> = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = ticker.tick() => {

                    ctl.heartbeat().await?;
                },
                v = rx2.recv() => {
                    if let Ok(Command::Shutdown) = v {
                        break;
                    }
                }
            }
        }
        ctl.stop().await?;
        Ok(())
    });

    let mut settings = config::Config::default();
    settings
        // Add in `./settings.toml`
        .merge(config::File::with_name("settings"))?
        // Add in settings from the environment (with a prefix of APEX)
        // Eg.. `APEX_DEBUG=1 ./target/app` would set the `debug` key
        .merge(config::Environment::with_prefix("APEX_"))?;

    #[cfg(feature = "simulator")]
    let mut device = Simulator::connect(tx.clone());

    device.clear().await?;

    let mut scheduler = Scheduler::new(device);

    ctrlc::set_handler(move || {
        info!("Ctrl + C received, shutting down!");
        tx.send(Command::Shutdown)
            .expect("Failed to send shutdown signal!");
    })?;

    scheduler.start(rx, settings).await?;
    #[cfg(all(feature = "usb", target_family = "unix"))]
    drop(hkm);

    handle.abort();

    Ok(())
}
