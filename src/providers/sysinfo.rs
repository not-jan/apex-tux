use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use num_traits::{pow, Pow};

use config::Config;
use embedded_graphics::{
    geometry::Point,
    mono_font::{iso_8859_15, MonoTextStyle},
    pixelcolor::BinaryColor,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
};
use futures::Stream;
use linkme::distributed_slice;
use log::{info, warn};
use tokio::{
    time,
    time::{Duration, MissedTickBehavior},
};

use sysinfo::{
    ComponentExt, CpuExt, CpuRefreshKind, NetworkData, NetworkExt, NetworksExt, RefreshKind,
    System, SystemExt,
};

#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

fn tick() -> i64 {
    chrono::offset::Utc::now().timestamp_millis()
}

#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Sysinfo display source.");

    let refreshes = RefreshKind::new()
        .with_cpu(CpuRefreshKind::everything())
        .with_components_list()
        .with_components()
        .with_networks_list()
        .with_networks()
        .with_memory();
    let sys = System::new_with_specifics(refreshes);

    let tick = tick();
    let last_tick = 0;

    let net_interface_name = config
        .get_str("sysinfo.net_interface_name")
        .unwrap_or("eth0".to_string());

    if sys
        .networks()
        .iter()
        .find(|(name, _)| **name == net_interface_name)
        .is_none()
    {
        warn!("Couldn't find network interface `{}`", net_interface_name);
        info!("Instead, found those interfaces:");
        for (interface_name, _) in sys.networks() {
            info!("\t{}", interface_name);
        }
    }

    let sensor_name = config
        .get_str("sysinfo.sensor_name")
        .unwrap_or("hwmon0 CPU Temperature".to_string());

    if sys
        .components()
        .iter()
        .find(|component| component.label() == sensor_name)
        .is_none()
    {
        warn!("Couldn't find sensor `{}`", sensor_name);
        info!("Instead, found those sensors:");
        for component in sys.components() {
            info!("\t{:?}", component);
        }
    }

    Ok(Box::new(Sysinfo {
        sys,
        tick,
        last_tick,
        refreshes,
        polling_interval: config.get_int("sysinfo.polling_interval").unwrap_or(2000) as u64,
        net_load_max: config.get_float("sysinfo.net_load_max").unwrap_or(100.0),
        cpu_frequency_max: config.get_float("sysinfo.cpu_frequency_max").unwrap_or(7.0),
        temperature_max: config.get_float("sysinfo.temperature_max").unwrap_or(100.0),
        net_interface_name,
        sensor_name,
    }))
}

struct Sysinfo {
    sys: System,
    refreshes: RefreshKind,

    tick: i64,
    last_tick: i64,

    polling_interval: u64,

    net_load_max: f64,
    cpu_frequency_max: f64,
    temperature_max: f64,

    net_interface_name: String,
    sensor_name: String,
}

impl Sysinfo {
    pub fn render(&mut self) -> Result<FrameBuffer> {
        self.poll();

        let load = self.sys.global_cpu_info().cpu_usage() as f64;
        let freq = self.sys.global_cpu_info().frequency() as f64 / 1000.0;
        let mem_used = self.sys.used_memory() as f64 / pow(1024, 3) as f64;

        let mut buffer = FrameBuffer::new();

        self.render_stat(0, &mut buffer, format!("C: {:>4.0}%", load), load / 100.0)?;
        self.render_stat(
            1,
            &mut buffer,
            format!("F: {:>4.2}G", freq),
            freq / self.cpu_frequency_max,
        )?;
        self.render_stat(
            2,
            &mut buffer,
            format!("M: {:>4.1}G", mem_used),
            self.sys.used_memory() as f64 / self.sys.total_memory() as f64,
        )?;

        if let Some(n) = self
            .sys
            .networks()
            .iter()
            .find(|(name, _)| **name == self.net_interface_name)
            .map(|t| t.1)
        {
            let net_direction = if n.received() > n.transmitted() {
                "I"
            } else {
                "O"
            };

            let (net_load, net_load_power, net_load_unit) = self.calculate_max_net_rate(n);
            let mut adjusted_net_load = format!(
                "{:.4}",
                (net_load / 1024_f64.pow(net_load_power)).to_string()
            );

            if adjusted_net_load.ends_with(".") {
                adjusted_net_load = adjusted_net_load.replace(".", "");
            }

            let _ = self.render_stat(
                3,
                &mut buffer,
                format!(
                    "{}: {:>4}{}",
                    net_direction, adjusted_net_load, net_load_unit
                ),
                net_load / (self.net_load_max * 1024_f64.pow(2)),
            );
        };

        if let Some(c) = self
            .sys
            .components()
            .iter()
            .find(|component| component.label() == self.sensor_name)
        {
            let _ = self.render_stat(
                4,
                &mut buffer,
                format!("T: {:>4.1}C", c.temperature()),
                c.temperature() as f64 / self.temperature_max,
            );
        }

        Ok(buffer)
    }

    fn calculate_max_net_rate(&self, net: &NetworkData) -> (f64, i32, &str) {
        let max_diff = std::cmp::max(net.received(), net.transmitted()) as f64;
        let max_rate = max_diff / ((self.tick - self.last_tick) as f64 / 1000.0);

        match max_rate {
            r if r > 1024_f64.pow(3) => (r, 3, "G"),
            r if r > 1024_f64.pow(2) => (r, 2, "M"),
            r if r > 1024_f64.pow(1) => (r, 1, "k"),
            r => (r, 0, "B"),
        }
    }

    fn poll(&mut self) {
        self.sys.refresh_specifics(self.refreshes);

        self.last_tick = self.tick;
        self.tick = tick();
    }

    fn render_stat(
        &self,
        slot: i32,
        buffer: &mut FrameBuffer,
        text: String,
        fill: f64,
    ) -> Result<()> {
        let style = MonoTextStyle::new(&iso_8859_15::FONT_4X6, BinaryColor::On);
        let metrics = style.measure_string(&text, Point::zero(), Baseline::Top);

        let slot_y = slot * 8 + 1;

        Text::with_baseline(&text, Point::new(0, slot_y), style, Baseline::Top).draw(buffer)?;

        let bar_start: i32 = metrics.bounding_box.size.width as i32 + 2;
        let border_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let fill_style = PrimitiveStyle::with_fill(BinaryColor::On);
        let fill_width = if fill.is_infinite() {
            0
        } else {
            (fill * (127 - bar_start) as f64).floor() as i32
        };

        Rectangle::with_corners(Point::new(bar_start, slot_y), Point::new(127, slot_y + 6))
            .into_styled(border_style)
            .draw(buffer)?;

        Rectangle::with_corners(
            Point::new(bar_start + 1, slot_y + 1),
            Point::new(bar_start + fill_width, slot_y + 5),
        )
        .into_styled(fill_style)
        .draw(buffer)?;

        Ok(())
    }
}

impl ContentProvider for Sysinfo {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        let mut interval = time::interval(Duration::from_millis(self.polling_interval));
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
        "sysinfo"
    }
}
