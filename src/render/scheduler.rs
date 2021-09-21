use crate::render::display::FrameBuffer;
use anyhow::{anyhow, Result};
pub const TICK_LENGTH: usize = 50;
pub const TICKS_PER_SECOND: usize = 1000 / TICK_LENGTH;
use crate::{
    hardware::device::Device,
    render::{
        display::ContentProvider,
        notifications::{Notification, NotificationProvider},
        stream::multiplex,
    },
};
use config::Config;
use futures::{pin_mut, stream, stream::Stream, StreamExt};
use itertools::Itertools;
use linkme::distributed_slice;
use log::{error, info};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::mpsc;

#[distributed_slice]
pub static CONTENT_PROVIDERS: [fn(&Config) -> Result<Box<dyn ContentWrapper>>] = [..];

#[distributed_slice]
pub static NOTIFICATION_PROVIDERS: [fn() -> Result<Box<dyn NotificationWrapper>>] = [..];

pub trait NotificationWrapper {
    fn proxy_stream<'a>(&'a mut self) -> Result<Box<dyn Stream<Item = Result<Notification>> + 'a>>;
}

impl<T: NotificationProvider> NotificationWrapper for T {
    fn proxy_stream<'this>(
        &'this mut self,
    ) -> Result<Box<dyn Stream<Item = Result<Notification>> + 'this>> {
        let x = <T as NotificationProvider>::stream(self)?;
        Ok(Box::new(x.fuse()))
    }
}

pub trait ContentWrapper {
    fn proxy_stream<'a>(&'a mut self) -> Result<Box<dyn Stream<Item = Result<FrameBuffer>> + 'a>>;
    fn provider_name(&self) -> &'static str;
}

impl<T: ContentProvider> ContentWrapper for T {
    fn proxy_stream<'this>(
        &'this mut self,
    ) -> Result<Box<dyn Stream<Item = Result<FrameBuffer>> + 'this>> {
        let x = <T as ContentProvider>::stream(self)?;
        Ok(Box::new(x.fuse()))
    }

    fn provider_name(&self) -> &'static str {
        self.name()
    }
}

pub struct Scheduler<T: Device> {
    device: T,
}

#[derive(Debug, Copy, Clone)]
pub enum Command {
    PreviousSource,
    NextSource,
    Shutdown,
}

impl<T: Device> Scheduler<T> {
    pub fn new(device: T) -> Self {
        Self { device }
    }

    pub async fn start(&mut self, rx: mpsc::Receiver<Command>, mut config: Config) -> Result<()> {
        let mut providers = CONTENT_PROVIDERS
            .iter()
            .map(|f| (f)(&mut config))
            .collect::<Result<Vec<_>>>()?;

        let mut notifications = NOTIFICATION_PROVIDERS
            .iter()
            .map(|f| (f)())
            .collect::<Result<Vec<_>>>()?;

        let (notifications, errors): (Vec<_>, Vec<_>) = notifications
            .iter_mut()
            .map(|s| s.proxy_stream().map(Box::into_pin))
            .partition_result();

        for e in errors {
            error!("{}", e);
        }

        let mut notifications = stream::select_all(notifications.into_iter());
        let current = Arc::new(AtomicUsize::new(0));
        info!("Found {} registered providers", providers.len());

        //let commands = from_receiver(rx).fuse();
        //pin_mut!(commands);
        pin_mut!(rx);

        let (providers, errors): (Vec<_>, Vec<_>) = providers
            .iter_mut()
            .map(|i| (i.provider_name(), i.proxy_stream()))
            .filter(|(name, _)| {
                let key = format!("{}.enabled", name);
                config.get_bool(&key).unwrap_or(true)
            })
            .map(|(name, i)| {
                i.map_err(|e| anyhow!("Failed to initalize provider: {}. Error: {}", name, e))
            })
            .partition_result();

        for e in errors {
            error!("{}", e);
        }

        let providers = providers
            .into_iter()
            .into_iter()
            .map(Box::into_pin)
            .map(futures::StreamExt::fuse)
            .collect::<Vec<_>>();
        let size = providers.len();
        let z = current.clone();

        let mut y = multiplex(providers, move || z.load(Ordering::SeqCst));
        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    println!("Handling command queue!");
                    match cmd {
                        Some(Command::Shutdown) => break,
                        Some(Command::NextSource) => {
                            let new = current.load(Ordering::SeqCst).wrapping_add(1) % size;
                            current.store(new, Ordering::SeqCst);
                            self.device.clear()?;
                        },
                        Some(Command::PreviousSource) => {
                            let new = match current.load(Ordering::SeqCst) {
                                0 => size - 1,
                                n => (n - 1) % size
                            };
                            current.store(new, Ordering::SeqCst);
                            self.device.clear()?;
                        },
                        _ => {}
                    }
                },
                notification = notifications.next() => {
                    if let Some(Ok(mut notification)) = notification {
                        let mut stream = Box::pin(notification.stream()?);
                        while let Some(display) = stream.next().await {
                            self.device.draw(&display?)?;
                        }
                    }
                }
                content = y.next() => {
                    if let Some(Ok(content)) = &content {
                        self.device.draw(content)?;
                    }
                }
            };
        }

        self.device.clear()?;
        Ok(())
    }
}
