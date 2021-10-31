use crate::render::display::{ContentProvider, FrameBuffer};
use anyhow::{anyhow, Result};
use async_stream::try_stream;
use embedded_graphics::{
    geometry::Size,
    image::Image,
    pixelcolor::BinaryColor,
    prelude::{Point, Primitive},
    primitives::{Line, PrimitiveStyle},
    Drawable,
};
use futures_core::stream::Stream;
use linkme::distributed_slice;

use async_stream::stream;
use log::info;
use tinybmp::Bmp;
use tokio::time;

use crate::render::{
    scheduler::CONTENT_PROVIDERS,
    text::{ScrollableBuilder, StatefulScrollable},
};

use crate::render::scheduler::ContentWrapper;
use config::Config;
use dbus::{message::MatchRule, nonblock, nonblock::SyncConnection};
use dbus_tokio::connection;
use embedded_graphics::{
    mono_font::{ascii, MonoTextStyle},
    text::{Baseline, Text},
};
use futures::StreamExt;
use std::{convert::TryInto, lazy::SyncLazy, sync::Arc};
use tokio::{
    task::JoinHandle,
    time::{Duration, MissedTickBehavior},
};

use crate::generated::mpris2_player::MediaPlayer2Player;
use dbus::{arg::PropMap, nonblock::Proxy, strings::BusName};
use futures::pin_mut;
use std::mem::drop;

static NOTE_ICON: &[u8] = include_bytes!("./../../assets/note.bmp");
static PAUSE_ICON: &[u8] = include_bytes!("./../../assets/pause.bmp");

static PAUSE_BMP: SyncLazy<Bmp<BinaryColor>> = SyncLazy::new(|| {
    Bmp::<BinaryColor>::from_slice(PAUSE_ICON).expect("Failed to parse BMP for pause icon!")
});

static NOTE_BMP: SyncLazy<Bmp<BinaryColor>> = SyncLazy::new(|| {
    Bmp::<BinaryColor>::from_slice(NOTE_ICON).expect("Failed to parse BMP for note icon!")
});

static PLAYER_TEMPLATE: SyncLazy<FrameBuffer> = SyncLazy::new(|| {
    let mut base = FrameBuffer::new();

    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    let points = vec![
        (Point::new(0, 39), Point::new(127, 39)),
        (Point::new(0, 39), Point::new(0, 39 - 5)),
        (Point::new(127, 39), Point::new(127, 39 - 5)),
    ];

    // Draw a border for the progress bar
    points
        .iter()
        .try_for_each(|(first, second)| {
            Line::new(*first, *second)
                .into_styled(style)
                .draw(&mut base)
        })
        .expect("Failed to prepare template image for music player!");
    base
});

static PLAY_TEMPLATE: SyncLazy<FrameBuffer> = SyncLazy::new(|| {
    let mut base = *PLAYER_TEMPLATE;
    Image::new(&*NOTE_BMP, Point::new(5, 5))
        .draw(&mut base)
        .expect("Failed to prepare 'play' template for music player");
    base
});

static PAUSE_TEMPLATE: SyncLazy<FrameBuffer> = SyncLazy::new(|| {
    let mut base = *PLAYER_TEMPLATE;
    Image::new(&*PAUSE_BMP, Point::new(5, 5))
        .draw(&mut base)
        .expect("Failed to prepare 'pause' template for music player");
    base
});

static IDLE_TEMPLATE: SyncLazy<FrameBuffer> = SyncLazy::new(|| {
    let mut base = *PAUSE_TEMPLATE;
    let style = MonoTextStyle::new(&ascii::FONT_6X10, BinaryColor::On);
    Text::with_baseline(
        "No player found",
        Point::new(5 + 3 + 24, 3),
        style,
        Baseline::Top,
    )
    .draw(&mut base)
    .expect("Failed to prepare 'idle' template for music player");
    base
});

static UNKNOWN_TITLE: &str = "Unknown title";
static UNKNOWN_ARTIST: &str = "Unknown artist";

const RECONNECT_DELAY: u64 = 5;

#[distributed_slice(CONTENT_PROVIDERS)]
static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering MPRIS2 display source.");

    let player = match config.get_str("mpris2.preferred_player") {
        Ok(name) => MediaPlayerBuilder::new().with_player_name(name),
        Err(_) => MediaPlayerBuilder::new(),
    };

    Ok(Box::new(player))
}

#[derive(Debug, Clone, Default)]
pub struct MediaPlayerBuilder {
    /// If a preference for the player is wanted specify this field
    name: Option<Arc<String>>,
}

pub struct MPRIS2 {
    handle: JoinHandle<()>,
    conn: Arc<SyncConnection>,
}

// Ok so the plan for the MPRIS2 module is to wait for two DBUS events
// - PropertiesChanged to see if the song changed
// - Seeked to see if the progress was changed manually
// There's an existing mpris2 crate but it doesn't support async operation which
// is kind of painful to use in this architecture.
// When we received these events they should be mapped and put into another
// queue. Upon receiving the event our code should pull the metadata from the
// player.

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Seeked,
    Properties,
    Timer,
}

#[derive(Debug)]
pub struct _Metadata(PropMap);

impl _Metadata {
    pub fn title(&self) -> Result<String> {
        ::dbus::arg::prop_cast::<String>(&self.0, "xesam:title")
            .cloned()
            .ok_or_else(|| anyhow!("Couldn't get title!"))
    }

    pub fn artists(&self) -> Result<String> {
        Ok(
            ::dbus::arg::prop_cast::<Vec<String>>(&self.0, "xesam:artist")
                .ok_or_else(|| anyhow!("Couldn't get artist!"))?
                .join(", "),
        )
    }

    pub fn length(&self) -> Result<i64> {
        ::dbus::arg::prop_cast::<i64>(&self.0, "mpris:length")
            .copied()
            .ok_or_else(|| anyhow!("Couldn't get length!"))
    }
}

#[derive(Copy, Clone, Debug)]
pub enum PlaybackStatus {
    Stopped,
    Paused,
    Playing,
}

pub struct Progress {
    metadata: _Metadata,
    position: i64,
    status: PlaybackStatus,
}

#[derive(Clone)]
pub struct Player<'a>(Proxy<'a, Arc<SyncConnection>>);

impl<'a> Player<'a> {
    pub fn new(path: impl Into<BusName<'a>>, conn: Arc<SyncConnection>) -> Self {
        Self(nonblock::Proxy::new(
            path.into(),
            "/org/mpris/MediaPlayer2",
            Duration::from_secs(2),
            conn,
        ))
    }

    pub async fn metadata(&self) -> Result<_Metadata> {
        Ok(_Metadata(self.0.metadata().await?))
    }

    pub async fn position(&self) -> Result<i64> {
        Ok(self.0.position().await?)
    }

    pub async fn progress(&self) -> Result<Progress> {
        Ok(Progress {
            metadata: self.metadata().await?,
            position: self.position().await?,
            status: self.playback_status().await?,
        })
    }

    pub fn name(&self) -> String {
        self.0.destination.to_string()
    }

    pub async fn playback_status(&self) -> Result<PlaybackStatus> {
        let status = self.0.playback_status().await?;

        match status.as_str() {
            "Playing" => Ok(PlaybackStatus::Playing),
            "Paused" => Ok(PlaybackStatus::Paused),
            "Stopped" => Ok(PlaybackStatus::Stopped),
            _ => Err(anyhow!("Bad playback status!")),
        }
    }
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
                        if let Some(_) = msg {
                            yield PlayerEvent::Seeked;
                        }
                    },
                    msg = meta_stream.next() => {
                        if let Some(_) = msg {
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
        let proxy = nonblock::Proxy::new(
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

#[derive(Debug, Clone)]
pub struct MediaPlayerRenderer {
    artist: StatefulScrollable,
    title: StatefulScrollable,
}

impl MediaPlayerRenderer {
    fn new() -> Result<Self> {
        let artist = ScrollableBuilder::new()
            .with_text(UNKNOWN_ARTIST)
            .with_custom_spacing(10)
            .with_position(Point::new(5 + 3 + 24, 3 + 10))
            .with_projection(Size::new(16 * 6, 10));
        let title = ScrollableBuilder::new()
            .with_text(UNKNOWN_TITLE)
            .with_custom_spacing(10)
            .with_position(Point::new(5 + 3 + 24, 3))
            .with_projection(Size::new(16 * 6, 10));

        Ok(Self {
            artist: artist.try_into()?,
            title: title.try_into()?,
        })
    }

    pub fn update(&mut self, progress: &Progress) -> Result<FrameBuffer> {
        let mut display = match progress.status {
            PlaybackStatus::Playing => *PLAY_TEMPLATE,
            PlaybackStatus::Paused | PlaybackStatus::Stopped => *PAUSE_TEMPLATE,
        };

        let metadata = &progress.metadata;
        let length = metadata
            .length()
            .map_err(|_| anyhow!("Couldn't get length!"))? as f64;

        let current = progress.position as f64;

        let completion = (current / length).clamp(0_f64, 1_f64);

        let pixels = (128_f64 - 2_f64 * 3_f64) * completion;
        let style = PrimitiveStyle::with_stroke(BinaryColor::On, 3);
        Line::new(Point::new(3, 35), Point::new(pixels as i32 + 3, 35))
            .into_styled(style)
            .draw(&mut display)?;

        let artists = metadata.artists()?;
        let title = metadata.title()?;

        if let Ok(false) = self.artist.update(&artists) {
            if artists.len() > 16 {
                self.artist.text.scroll();
            }
        }

        if let Ok(false) = self.title.update(&title) {
            if title.len() > 16 {
                self.title.text.scroll();
            }
        }

        self.title.text.draw(&mut display)?;
        self.artist.text.draw(&mut display)?;

        Ok(display)
    }
}

impl MediaPlayerBuilder {
    pub fn with_player_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(Arc::new(name.into()));
        self
    }

    pub fn new() -> Self {
        Self::default()
    }
}

impl ContentProvider for MediaPlayerBuilder {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        info!(
            "Trying to connect to DBUS with player preference: {:?}",
            self.name
        );

        let mut renderer = MediaPlayerRenderer::new()?;

        Ok(try_stream! {
            let mpris = MPRIS2::new().await?;
            pin_mut!(mpris);

            let mut interval = time::interval(Duration::from_secs(RECONNECT_DELAY));
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            'outer: loop {
                info!(
                    "Trying to connect to DBUS with player preference: {:?}",
                    self.name
                );
                yield *IDLE_TEMPLATE;
                let player = mpris.wait_for_player(self.name.clone()).await?;

                info!("Connected to music player: {:?}", player.name());


                let tracker = mpris.stream().await?;
                pin_mut!(tracker);

                while let Some(_) = tracker.next().await {
                    // TODO: We could probably save *some* resources here by making use of the event
                    // that's being called but I don't see enough of a reason to do so at the moment
                    if let Ok(progress) = player.progress().await {
                        if let Ok(image) = renderer.update(&progress) {
                            yield image;
                        }
                    } else {
                        continue 'outer;
                    }
                }
            }
        })
    }

    fn name(&self) -> &'static str {
        "mpris2"
    }
}
