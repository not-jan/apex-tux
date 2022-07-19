use crate::{
    render::{
        notifications::{Icon, Notification, NotificationBuilder, NotificationProvider},
        scheduler::NotificationWrapper,
    },
    scheduler::NOTIFICATION_PROVIDERS,
};
use anyhow::{anyhow, Result};
use async_stream::try_stream;
use dbus::{
    arg::messageitem::MessageItem,
    channel::MatchingReceiver,
    message::MatchRule,
    nonblock,
    strings::{Interface, Member},
    Message,
};
use dbus_tokio::connection;
use embedded_graphics::pixelcolor::BinaryColor;
use futures::{channel::mpsc, StreamExt};
use futures_core::Stream;
use lazy_static::lazy_static;
use linkme::distributed_slice;
use log::{debug, info};
use std::{convert::TryFrom, time::Duration};
use tinybmp::Bmp;

#[distributed_slice(NOTIFICATION_PROVIDERS)]
static PROVIDER_INIT: fn() -> Result<Box<dyn NotificationWrapper>> = register_callback;

#[allow(clippy::unnecessary_wraps)]
fn register_callback() -> Result<Box<dyn NotificationWrapper>> {
    info!("Registering DBUS notification source.");
    let dbus = Box::new(Dbus {});
    Ok(dbus)
}

static DISCORD_ICON: &[u8] = include_bytes!("./../../assets/discord.bmp");
lazy_static! {
    static ref DISCORD_ICON_BMP: Bmp<'static, BinaryColor> =
        Bmp::<BinaryColor>::from_slice(DISCORD_ICON).expect("Failed to parse BMP");
}

pub struct Dbus {}

enum NotificationType {
    Discord { title: String, content: String },
    Unsupported,
}

impl NotificationType {
    pub fn render(&self) -> Result<Notification> {
        let builder = NotificationBuilder::new();

        match self {
            NotificationType::Discord { title, content } => {
                let icon = Icon::new(*DISCORD_ICON_BMP);
                builder
                    .with_icon(icon)
                    .with_content(content)
                    .with_title(title)
                    .build()
            }
            NotificationType::Unsupported => Err(anyhow!("Unsupported notification type!")),
        }
    }
}

impl TryFrom<Message> for NotificationType {
    type Error = anyhow::Error;

    fn try_from(value: Message) -> Result<Self, Self::Error> {
        let source = value.get_source()?;

        Ok(match source.as_str() {
            "discord" => {
                let (_, _, _, title, content) =
                    value.read5::<String, u32, String, String, String>()?;
                if let Some(MessageItem::Dict(dict)) = value.get_items().get(6) {
                    if let Some((MessageItem::Str(key), _)) = dict.last() {
                        if key != "sender-pid" {
                            return Ok(NotificationType::Unsupported);
                        }
                    }
                }

                NotificationType::Discord { title, content }
            }
            _ => NotificationType::Unsupported,
        })
    }
}

trait MessageExt {
    fn get_source(&self) -> Result<String>;
}

impl MessageExt for Message {
    fn get_source(&self) -> Result<String> {
        self.get1::<String>()
            .ok_or_else(|| anyhow!("Couldn't get source"))
    }
}

impl NotificationProvider for Dbus {
    type NotificationStream<'a> = impl Stream<Item = Result<Notification>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::NotificationStream<'this>> {
        let mut rule = MatchRule::new();
        rule.interface = Some(Interface::from("org.freedesktop.Notifications"));
        rule.member = Some(Member::from("Notify"));

        let (resource, conn) = connection::new_session_sync()?;

        tokio::spawn(async {
            let err = resource.await;
            panic!("Lost connection to D-Bus: {}", err);
        });

        let (mut tx, mut rx) = mpsc::channel(10);

        tokio::spawn(async move {
            let conn2 = conn.clone();

            let proxy = nonblock::Proxy::new(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                Duration::from_millis(5000),
                conn,
            );

            // `BecomeMonitor` is the modern approach to monitoring messages on the bus
            // There used to be `eavesdrop` but it's since been deprecated and seeing as the
            // change happened back in 2017 I've elected for not supporting that
            // here.
            proxy
                .method_call(
                    "org.freedesktop.DBus.Monitoring",
                    "BecomeMonitor",
                    (vec![rule.match_str()], 0_u32),
                )
                .await?;

            conn2.start_receive(
                rule,
                Box::new(move |msg, _| {
                    debug!("DBus event from {:?}", msg.sender());
                    tx.try_send(msg).is_ok()
                }),
            );

            Ok::<(), anyhow::Error>(())
        });

        Ok(try_stream! {
             while let Some(msg) = rx.next().await {
                let ty = NotificationType::try_from(msg)?;

                if let NotificationType::Unsupported = &ty {
                    continue;
                } else {
                    if let Ok(notif) = ty.render() {
                        yield notif;
                    }
                }
            }
            println!("WTF?");
        })
    }
}
