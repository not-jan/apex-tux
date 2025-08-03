use crate::generated::MediaPlayer2Player;
use anyhow::{anyhow, Result};
use apex_music::{AsyncPlayer, Metadata as MetadataTrait, PlaybackStatus, PlayerEvent, Progress};
use async_stream::stream;
use dbus::{
    arg::PropMap,
    message::MatchRule,
    nonblock::{Proxy, SyncConnection},
    strings::BusName,
};
use dbus_tokio::connection;
use futures_core::stream::Stream;
use futures_util::StreamExt;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::{task::JoinHandle, time, time::MissedTickBehavior};

#[derive(Clone)]
pub struct Player<'a>(Proxy<'a, Arc<SyncConnection>>);

#[derive(Debug)]
pub struct Metadata(PropMap);

impl Metadata {
    fn length_<T: Copy + Sized + 'static>(&self) -> Result<T> {
        ::dbus::arg::prop_cast::<T>(&self.0, "mpris:length")
            .copied()
            .ok_or_else(|| anyhow!("Couldn't get length!"))
    }
}

impl MetadataTrait for Metadata {
    fn title(&self) -> Result<String> {
        ::dbus::arg::prop_cast::<String>(&self.0, "xesam:title")
            .cloned()
            .ok_or_else(|| anyhow!("Couldn't get title!"))
    }

    fn artists(&self) -> Result<String> {
        Ok(
            ::dbus::arg::prop_cast::<Vec<String>>(&self.0, "xesam:artist")
                .ok_or_else(|| anyhow!("Couldn't get artist!"))?
                .join(", "),
        )
    }

    fn length(&self) -> Result<u64> {
        match (self.length_::<i64>(), self.length_::<u64>()) {
            (_, Ok(val)) => Ok(val),
            (Ok(val), _) => Ok(val as u64),
            (_, _) => Err(anyhow!("Couldn't get length!")),
        }
    }
}

pub struct MPRIS2 {
    handle: JoinHandle<()>,
    conn: Arc<SyncConnection>,
}

impl MPRIS2 {
    pub async fn new() -> Result<Self> {
        let (resource, conn) = connection::new_session_sync()?;

        let handle = tokio::spawn(async {
            let err = resource.await;
            panic!("Lost connection to D-Bus: {}", err);
        });

        Ok(Self { handle, conn })
    }

    #[allow(unreachable_code, unused_variables)]
    pub async fn stream(&self) -> Result<impl Stream<Item = PlayerEvent>> {
        let mr = MatchRule::new()
            .with_path("/org/mpris/MediaPlayer2")
            .with_interface("org.freedesktop.DBus.Properties")
            .with_member("PropertiesChanged");

        let (meta_match, mut meta_stream) = self.conn.add_match(mr).await?.msg_stream();

        let mr = MatchRule::new()
            .with_interface("org.mpris.MediaPlayer2.Player")
            .with_path("/org/mpris/MediaPlayer2")
            .with_member("Seeked");

        let (seek_match, mut seek_stream) = self.conn.add_match(mr).await?.msg_stream();

        Ok(stream! {
            loop {
                let mut timer = time::interval(time::Duration::from_millis(100));
                timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
                // First timer tick elapses instantaneously
                timer.tick().await;

                tokio::select! {
                    msg = seek_stream.next() => {
                        if msg.is_some() {
                            yield PlayerEvent::Seeked;
                        }
                    },
                    msg = meta_stream.next() => {
                        if msg.is_some() {
                            yield PlayerEvent::Properties;
                        }
                    },
                    _ = timer.tick() => {
                        yield PlayerEvent::Timer;
                    }
                }
            }
            // The signal handler will unregister if those two are dropped so we never drop them ;)
            drop(seek_match);
            drop(meta_match);
        })
    }

    pub async fn list_names(&self) -> Result<Vec<String>> {
        let proxy = Proxy::new(
            "org.freedesktop.DBus",
            "/",
            Duration::from_secs(2),
            self.conn.clone(),
        );

        let (result,): (Vec<String>,) = proxy
            .method_call("org.freedesktop.DBus", "ListNames", ())
            .await?;

        let result = result
            .iter()
            .filter(|name| name.starts_with("org.mpris.MediaPlayer2."))
            .cloned()
            .collect::<Vec<_>>();

        Ok(result)
    }

    pub async fn wait_for_player(&self, name: Option<Arc<String>>) -> Result<Player<'_>> {
        let mut interval = time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let name = name.map(|n| n.to_string());

        // TODO: Instead of having a hard delay we might be able to wait on a
        // notification from DBus instead?

        loop {
            let names = self.list_names().await?;

            if let Some(name) = &name {
                // We have a player preference, let's check if it exists
                if let Some(player) = names.into_iter().find(|p| p.contains(name)) {
                    // Hell yeah, we found a player
                    return Ok(Player::new(player, self.conn.clone()));
                }
            } else {
                // Let's try to find a player that's either playing or paused
                for name in names {
                    let player = Player::new(name, self.conn.clone());

                    match player.playback_status().await {
                        // Something is playing or paused right now, let's use that
                        Ok(PlaybackStatus::Playing | PlaybackStatus::Paused) => {
                            return Ok(player);
                        }
                        // Stopped players could be remnants of browser tabs that were playing in
                        // the past but are dead now and we'd just get stuck here.
                        _ => {
                            continue;
                        }
                    }
                }
            }

            interval.tick().await;
        }
    }
}

impl Drop for MPRIS2 {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl<'a> Player<'a> {
    pub fn new(path: impl Into<BusName<'a>>, conn: Arc<SyncConnection>) -> Self {
        Self(Proxy::new(
            path.into(),
            "/org/mpris/MediaPlayer2",
            Duration::from_secs(2),
            conn,
        ))
    }

    pub async fn progress(&self) -> Result<Progress<Metadata>> {
        Ok(Progress {
            metadata: self.metadata().await?,
            position: self.position().await?,
            status: self.playback_status().await?,
        })
    }
}

impl<'a> AsyncPlayer for Player<'a> {
    type Metadata = Metadata;

    type MetadataFuture<'b>
        = impl Future<Output = Result<Self::Metadata>> + 'b
    where
        Self: 'b;
    type NameFuture<'b>
        = impl Future<Output = String> + 'b
    where
        Self: 'b;
    type PlaybackStatusFuture<'b>
        = impl Future<Output = Result<PlaybackStatus>> + 'b
    where
        Self: 'b;
    type PositionFuture<'b>
        = impl Future<Output = Result<i64>> + 'b
    where
        Self: 'b;

    #[allow(clippy::needless_lifetimes)]
    fn metadata<'this>(&'this self) -> Self::MetadataFuture<'this> {
        async { Ok(Metadata(self.0.metadata().await?)) }
    }

    #[allow(clippy::needless_lifetimes)]
    fn playback_status<'this>(&'this self) -> Self::PlaybackStatusFuture<'this> {
        async {
            let status = self.0.playback_status().await?;

            match status.as_str() {
                "Playing" => Ok(PlaybackStatus::Playing),
                "Paused" => Ok(PlaybackStatus::Paused),
                "Stopped" => Ok(PlaybackStatus::Stopped),
                _ => Err(anyhow!("Bad playback status!")),
            }
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn name<'this>(&'this self) -> Self::NameFuture<'this> {
        async { self.0.destination.to_string() }
    }

    #[allow(clippy::needless_lifetimes)]
    fn position<'this>(&'this self) -> Self::PositionFuture<'this> {
        async { Ok(self.0.position().await?) }
    }
}
