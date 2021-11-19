use crate::render::display::ContentProvider;
#[cfg(not(target_os = "windows"))]
use anyhow::anyhow;
use anyhow::Result;
use async_stream::try_stream;
use embedded_graphics::{
    geometry::Size,
    image::Image,
    pixelcolor::BinaryColor,
    prelude::{Point, },

    Drawable,
};
#[cfg(not(target_os = "windows"))]
use embedded_graphics::prelude::Primitive;
#[cfg(not(target_os = "windows"))]
use embedded_graphics::primitives::{Line, PrimitiveStyle};
use futures_core::stream::Stream;
use linkme::distributed_slice;

use log::info;
use tinybmp::Bmp;
use tokio::time;

use crate::render::{
    scheduler::{ContentWrapper, CONTENT_PROVIDERS},
    text::{ScrollableBuilder, StatefulScrollable},
};
use apex_music::{AsyncPlayer, Metadata};
use config::Config;
use embedded_graphics::{
    mono_font::{ascii, MonoTextStyle},
    text::{Baseline, Text},
};
use futures::StreamExt;
use apex_music::Progress;
use std::{convert::TryInto, lazy::SyncLazy, sync::Arc};
use tokio::time::{Duration, MissedTickBehavior};

use apex_hardware::FrameBuffer;
use apex_music::PlaybackStatus;
use futures::pin_mut;

static NOTE_ICON: &[u8] = include_bytes!("./../../assets/note.bmp");
static PAUSE_ICON: &[u8] = include_bytes!("./../../assets/pause.bmp");

static PAUSE_BMP: SyncLazy<Bmp<BinaryColor>> = SyncLazy::new(|| {
    Bmp::<BinaryColor>::from_slice(PAUSE_ICON).expect("Failed to parse BMP for pause icon!")
});

static NOTE_BMP: SyncLazy<Bmp<BinaryColor>> = SyncLazy::new(|| {
    Bmp::<BinaryColor>::from_slice(NOTE_ICON).expect("Failed to parse BMP for note icon!")
});

// Windows doesn't expose the current progress within the song so we don't draw it here
// TODO: Spice this up?
#[cfg(target_os = "windows")]
static PLAYER_TEMPLATE: SyncLazy<FrameBuffer> = SyncLazy::new(|| {
    FrameBuffer::new()
});

#[cfg(not(target_os = "windows"))]
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

// Ok so the plan for the MPRIS2 module is to wait for two DBUS events
// - PropertiesChanged to see if the song changed
// - Seeked to see if the progress was changed manually
// There's an existing mpris2 crate but it doesn't support async operation which
// is kind of painful to use in this architecture.
// When we received these events they should be mapped and put into another
// queue. Upon receiving the event our code should pull the metadata from the
// player.

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

    pub fn update<T: Metadata>(&mut self, progress: &Progress<T>) -> Result<FrameBuffer> {
        let mut display = match progress.status {
            PlaybackStatus::Playing => *PLAY_TEMPLATE,
            PlaybackStatus::Paused | PlaybackStatus::Stopped => *PAUSE_TEMPLATE,
        };

        let metadata = &progress.metadata;

        #[cfg(not(target_os = "windows"))]
        {
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
        }

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
            #[cfg(target_os = "windows")]
            let mpris = apex_windows::Player::new()?;
            #[cfg(target_os = "linux")]
            let mpris = apex_mpris2::MPRIS2::new().await?;
            pin_mut!(mpris);

            let mut interval = time::interval(Duration::from_secs(RECONNECT_DELAY));
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            'outer: loop {
                info!(
                    "Trying to connect to DBUS with player preference: {:?}",
                    self.name
                );
                yield *IDLE_TEMPLATE;
                #[cfg(target_os = "windows")]
                let player = &mpris;
                #[cfg(target_os = "linux")]
                let player = mpris.wait_for_player(self.name.clone()).await?;

                info!("Connected to music player: {:?}", player.name().await);


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
