use crate::render::display::{ContentProvider, FrameBuffer};
use anyhow::{anyhow, Result};
use async_stream::try_stream;
use embedded_graphics::{
    geometry::{OriginDimensions, Point, Size},
    image::Image,
    pixelcolor::BinaryColor,
    Drawable,
};
use num_traits::AsPrimitive;

use crate::{
    dbus::notifications::ProgressBar,
    render::{
        scheduler::{TICKS_PER_SECOND, TICK_LENGTH},
        text::{Scrollable, ScrollableBuilder},
    },
};
use embedded_graphics::{
    mono_font::{ascii, MonoFont, MonoTextStyle},
    text::Text,
};
use futures_core::stream::Stream;
use tinybmp::Bmp;
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};

pub struct Notification {
    frame: FrameBuffer,
    ticks: u32,
    title: Scrollable,
    scroll: bool,
    content: String,
}

#[derive(Debug, Clone)]
pub struct Icon<'a>(Bmp<'a, BinaryColor>);

impl<'a> Icon<'a> {
    pub fn new(icon: Bmp<'a, BinaryColor>) -> Self {
        Self(icon)
    }
}

#[derive(Debug, Clone, Default)]
pub struct NotificationBuilder<'a> {
    title: Option<&'a str>,
    content: Option<String>,
    icon: Option<Icon<'a>>,
    font: Option<&'a MonoFont<'a>>,
}

pub trait NotificationProvider {
    type NotificationStream<'a>: Stream<Item = Result<Notification>> + 'a;

    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::NotificationStream<'this>>;
}

impl ContentProvider for Notification {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(TICK_LENGTH.as_()));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let origin = Point::new(117, 29);
        let progress = ProgressBar::new(origin, self.ticks as f32);

        // TODO: Remove hardcoded font
        let style = MonoTextStyle::new(&ascii::FONT_6X10, BinaryColor::On);

        Ok(try_stream! {
            for i in 0..self.ticks {
                let mut image = self.frame.clone();
                self.title.at_tick(&mut image, if self.scroll {
                    i
                } else {
                    0
                })?;
                Text::new(&self.content, Point::new(3 + 24, 10 + 10), style).draw(&mut image)?;
                progress.draw_at(i as f32, &mut image)?;
                yield image;
                interval.tick().await;
            }
        })
    }

    fn name(&self) -> &'static str {
        "notification"
    }
}

impl<'a> NotificationBuilder<'a> {
    pub fn new() -> Self {
        NotificationBuilder::default()
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    pub fn with_icon(mut self, icon: Icon<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    fn title(&self) -> &'a str {
        self.title.unwrap_or("Notification")
    }

    fn font(&self) -> &'a MonoFont {
        self.font.unwrap_or(&ascii::FONT_6X10)
    }

    fn offset(&self) -> Size {
        self.icon
            .as_ref()
            .map_or_else(Size::zero, |icon| icon.0.size())
            + Size::new(3, 10)
    }

    fn projection(&self) -> Size {
        let offset = self.offset();
        let display_size = Size::new(128, 40);
        let height = self.font().character_size.height;
        let width = (display_size - offset).width - 3;

        Size::new(width, height)
    }

    fn projection_characters(&self) -> u32 {
        let font = self.font();
        let projection = self.projection();

        projection.width / font.character_size.width
    }

    fn needs_scroll(&self) -> bool {
        let length = self.title().len();
        (self.projection_characters() as usize) < length
    }

    fn required_ticks(&self) -> u32 {
        let title = self.title();
        let font = self.font();
        let scroll_time = if self.needs_scroll() {
            (title.len() - self.projection_characters() as usize + 2)
                * font.character_size.width as usize
        } else {
            0
        };

        (TICKS_PER_SECOND + scroll_time + TICKS_PER_SECOND).as_()
    }

    pub fn build(self) -> Result<Notification> {
        let mut base_image = FrameBuffer::new();

        // We have an icon so lets draw it
        if let Some(icon) = &self.icon {
            let Size { width, height } = icon.0.size();

            if width != 24 || height != 24 {
                return Err(anyhow!(
                    "Notification icons need to be 24x24 for the time being!"
                ));
            }

            Image::new(&icon.0, Point::zero()).draw(&mut base_image)?;
        }

        let size = self.offset();
        let projection = self.projection();
        let offset = Point::new(size.width.as_(), 3);

        let title = ScrollableBuilder::new()
            .with_text(self.title())
            .with_position(offset)
            .with_projection(projection)
            .build()?;

        Ok(Notification {
            frame: base_image,
            ticks: self.required_ticks(),
            title,
            scroll: self.needs_scroll(),
            content: self.content.unwrap_or_default(),
        })
    }
}
