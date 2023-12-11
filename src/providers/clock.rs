use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use chrono::{DateTime, Local};
use config::Config;
use embedded_graphics::{
    geometry::Point,
    mono_font::{iso_8859_15, MonoTextStyle},
    pixelcolor::BinaryColor,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
};
use futures::Stream;
use linkme::distributed_slice;
use log::info;
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};

#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[derive(Debug, Copy, Clone)]
/// Represents the options a user can choose for the clock format
enum ClockFormat {
    /// 12hr clock format with AM / PM
    Twelve,
    /// 24hr clock format (military time)
    TwentyFour,
    /// This setting will use the current locales clock format instead
    Locale,
}

#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Clock display source.");

    let clock_format = match config.get_bool("clock.twelve_hour") {
        Ok(true) => ClockFormat::Twelve,
        Ok(false) => ClockFormat::TwentyFour,
        _ => ClockFormat::Locale,
    };

    Ok(Box::new(Clock { clock_format }))
}

pub struct Clock {
    clock_format: ClockFormat,
}

impl Clock {
    pub fn render(&self) -> Result<FrameBuffer> {
        let local: DateTime<Local> = Local::now();
        let format_string = match self.clock_format {
            ClockFormat::Twelve => "%I:%M:%S %p",
            ClockFormat::TwentyFour => "%H:%M:%S",
            ClockFormat::Locale => "%X",
        };

        let text = local.format(format_string).to_string();
        let mut buffer = FrameBuffer::new();
        let style = MonoTextStyle::new(&iso_8859_15::FONT_8X13_BOLD, BinaryColor::On);
        let metrics = style.measure_string(&text, Point::zero(), Baseline::Top);
        let height: i32 = (metrics.bounding_box.size.height / 2) as i32;
        let width: i32 = (metrics.bounding_box.size.width / 2) as i32;

        Text::with_baseline(
            &text,
            Point::new(128 / 2 - width, 40 / 2 - height),
            style,
            Baseline::Top,
        )
        .draw(&mut buffer)?;

        Ok(buffer)
    }
}

impl ContentProvider for Clock {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    // This needs to be enabled until full GAT support is here
    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(50));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        Ok(try_stream! {
            loop {
                if let Ok(image) = self.render() {
                    yield image;
                }
                interval.tick().await;
            }
        })
    }

    fn name(&self) -> &'static str {
        "clock"
    }
}
