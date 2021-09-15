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
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder, Progress};

use log::info;
use tinybmp::Bmp;

use crate::render::{
    scheduler::CONTENT_PROVIDERS,
    text::{ScrollableBuilder, StatefulScrollable},
};

use crate::render::scheduler::ContentWrapper;
use std::{convert::TryInto, lazy::SyncLazy};

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

static UNKNOWN_TITLE: &str = "Unknown title";
static UNKNOWN_ARTIST: &str = "Unknown artist";

#[distributed_slice(CONTENT_PROVIDERS)]
static PROVIDER_INIT: fn() -> Result<Box<dyn ContentWrapper>> = register_callback;

#[allow(clippy::unnecessary_wraps)]
fn register_callback() -> Result<Box<dyn ContentWrapper>> {
    info!("Registering MPRIS2 display source.");
    let player = Box::new(MediaPlayerBuilder::new().with_player_name("Lollypop"));
    Ok(player)
}

pub struct MediaPlayerBuilder {
    /// If a preference for the player is wanted specify this field
    name: Option<String>,
    /// Interval to re-poll data about the music player in ms
    ticks: u32,
}

impl Default for MediaPlayerBuilder {
    fn default() -> Self {
        Self {
            name: None,
            ticks: 100,
        }
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
        let mut display = match progress.playback_status() {
            PlaybackStatus::Playing => *PLAY_TEMPLATE,
            PlaybackStatus::Paused | PlaybackStatus::Stopped => *PAUSE_TEMPLATE,
        };

        let metadata = progress.metadata();
        let length = progress
            .length()
            .ok_or_else(|| anyhow!("Couldn't get length!"))?;

        let current = progress.position();

        let completion = (current.as_secs_f64() / length.as_secs_f64()).clamp(0_f64, 1_f64);

        let pixels = (128_f64 - 2_f64 * 3_f64) * completion;
        let style = PrimitiveStyle::with_stroke(BinaryColor::On, 3);
        Line::new(Point::new(3, 35), Point::new(pixels as i32 + 3, 35))
            .into_styled(style)
            .draw(&mut display)?;

        let artists = metadata.printable_artists();
        let title = metadata.printable_title();

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
        self.name = Some(name.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_custom_interval(mut self, interval: u32) -> Self {
        self.ticks = interval;
        self
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn connect(&self) -> Result<Player> {
        let finder = PlayerFinder::new().map_err(|e| anyhow!(e))?;
        let player = match &self.name {
            Some(name) => finder
                .find_all()
                .map_err(|e| anyhow!(e))?
                .into_iter()
                .find(|player| player.identity() == name)
                .ok_or_else(|| anyhow!("Player {:?} not found!", name)),
            None => finder
                .find_active()
                .map_err(|_| anyhow!("No active player found!")),
        }?;

        Ok(player)
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

        let player = self.connect()?;
        let ticks = self.ticks;

        info!("Connected to music player: {:?}", player.identity());

        let mut renderer = MediaPlayerRenderer::new()?;

        Ok(try_stream! {
            // We get new meta data every 100ms to update our progress bar
            let mut tracker = player.track_progress(ticks).map_err(|e| anyhow!(e))?;

            loop {
                let progress = tracker.tick();

                if let Ok(image) = renderer.update(progress.progress) {
                    yield image;
                }

                if !player.is_running() {
                    // Clear the screen one last time
                    yield FrameBuffer::new();

                }
            }
        })
    }

    fn name(&self) -> &'static str {
        "mpris2"
    }
}

/// Helper trait to simplify collecting metadata from MPRIS2 players
trait MetadataExt {
    /// Collect all artists comma separated as a string or return a default
    /// value
    fn printable_artists(&self) -> String;
    /// Return the title of the current song or a default value
    fn printable_title(&self) -> String;
}

impl MetadataExt for Metadata {
    fn printable_artists(&self) -> String {
        self.artists()
            .unwrap_or_else(|| vec![UNKNOWN_ARTIST])
            .join(", ")
    }

    fn printable_title(&self) -> String {
        self.title().unwrap_or(UNKNOWN_TITLE).to_string()
    }
}
