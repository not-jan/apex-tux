use crate::{
    render::{
        display::ContentProvider,
        scheduler::ContentWrapper,
    },
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use config::Config;
use embedded_graphics::{
    geometry::Point,
    mono_font::{iso_8859_15::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
};
use futures::Stream;
use linkme::distributed_slice;
use log::{info, warn};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::time::{self, Duration, MissedTickBehavior};

const MAX_LINES: usize = 4;
const LINE_HEIGHT: i32 = 10;
const DISPLAY_WIDTH: u32 = 128;

#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering File display source.");

    let path = config
        .get_str("file.path")
        .unwrap_or_else(|_| String::from("text.txt"));
    let polling_interval = config
        .get_int("file.polling_interval")
        .unwrap_or(1000)
        .max(1) as u64;

    Ok(Box::new(FileText::new(
        PathBuf::from(path),
        polling_interval,
    )?))
}

pub struct FileText {
    path: PathBuf,
    polling_interval: u64,
    lines: Vec<String>,
    scroll_positions: Vec<u32>,
}

impl FileText {
    fn new(path: PathBuf, polling_interval: u64) -> Result<Self> {
        Ok(Self {
            path,
            polling_interval,
            lines: vec![String::new()],
            scroll_positions: vec![0],
        })
    }

    fn read(path: &Path) -> Result<Vec<String>> {
        let contents = fs::read_to_string(path)?;
        let mut lines = contents
            .lines()
            .take(MAX_LINES)
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Ok(lines)
    }

    fn render(&mut self) -> Result<FrameBuffer> {
        let mut buffer = FrameBuffer::new();
        let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

        for (index, line) in self.lines.iter().enumerate() {
            let width = style
                .measure_string(line, Point::zero(), Baseline::Top)
                .bounding_box
                .size
                .width;
            let scroll = if width > DISPLAY_WIDTH {
                let position = self.scroll_positions[index];
                let cycle = width + DISPLAY_WIDTH;
                let x = -((position % cycle) as i32);
                self.scroll_positions[index] = (position + 1) % cycle;
                x
            } else {
                self.scroll_positions[index] = 0;
                0
            };

            Text::with_baseline(
                line,
                Point::new(scroll, index as i32 * LINE_HEIGHT),
                style,
                Baseline::Top,
            )
            .draw(&mut buffer)?;
        }

        Ok(buffer)
    }

    fn refresh(&mut self) {
        match Self::read(&self.path) {
            Ok(lines) if lines != self.lines => {
                self.scroll_positions = vec![0; lines.len()];
                self.lines = lines;
            }
            Ok(_) => {}
            Err(err) => warn!("Failed to read file '{}': {err}", self.path.display()),
        }
    }
}

impl ContentProvider for FileText {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    fn stream(&mut self) -> Result<<Self as ContentProvider>::ContentStream<'_>> {
        let mut interval = time::interval(Duration::from_millis(50));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut last_read = Instant::now() - Duration::from_millis(self.polling_interval);

        Ok(try_stream! {
            loop {
                if last_read.elapsed() >= Duration::from_millis(self.polling_interval) {
                    self.refresh();
                    last_read = Instant::now();
                }
                yield self.render()?;
                interval.tick().await;
            }
        })
    }

    fn name(&self) -> &'static str {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::FileText;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn read_preserves_lines_and_limits_output() {
        let path = std::env::temp_dir().join(format!(
            "apex-tux-file-provider-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "first\r\nsecond\nthird\nfourth\nfifth\nsixth").unwrap();

        assert_eq!(
            FileText::read(&path).unwrap(),
            vec!["first", "second", "third", "fourth"]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn refresh_picks_up_file_changes() {
        let path = std::env::temp_dir().join(format!(
            "apex-tux-file-provider-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, "test1").unwrap();

        let mut provider = FileText::new(path.clone(), 1000).unwrap();
        provider.refresh();
        assert_eq!(provider.lines, vec!["test1"]);
        provider.render().unwrap();

        fs::write(&path, "test2").unwrap();
        provider.refresh();
        assert_eq!(provider.lines, vec!["test2"]);
        provider.render().unwrap();
        fs::remove_file(path).unwrap();
    }
}
