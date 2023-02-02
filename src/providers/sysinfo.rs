use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::Result;
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use num_traits::{ToPrimitive, pow, Pow};

use std::mem;

use config::Config;
use embedded_graphics::{
    primitives::{Rectangle, Primitive, PrimitiveStyle},
    geometry::Point,
    mono_font::{ascii, MonoTextStyle},
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

use apex_sysinfo::{
    get_cpufreq,
    get_hwmon_temp,
    sg_init,
    sg_shutdown,
    sg_cpu_percents,
    sg_get_cpu_percents_of,
    sg_get_cpu_stats_diff,
    sg_cpu_percent_source_sg_last_diff_cpu_percent,
    sg_mem_stats,
    sg_get_mem_stats,
    sg_network_io_stats,
    sg_get_network_io_stats_diff,
    sg_get_network_io_stats
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

    unsafe { sg_init(1); }

    let cpu : sg_cpu_percents = unsafe { mem::zeroed() };
    let mem : sg_mem_stats = unsafe { mem::zeroed() };
    let net : sg_network_io_stats = unsafe { mem::zeroed() };
    let tick = tick();
    let last_tick = 0;

    Ok(Box::new(Sysinfo {
        cpu, mem, net, tick, last_tick, temp: 0.0,
        polling_interval: config.get_int("sysinfo.polling_interval").unwrap_or(2000) as u64,
        net_interface_name: config.get_str("sysinfo.net_interface_name").unwrap_or("eth0".to_string()),
        hwmon_name: config.get_str("sysinfo.hwmon_name").unwrap_or("hwmon0".to_string()),
        hwmon_sensor_name: config.get_str("sysinfo.hwmon_sensor_name").unwrap_or("CPU Temperature".to_string()),
    }))
}

struct Sysinfo {
    cpu: sg_cpu_percents,
    mem: sg_mem_stats,
    net: sg_network_io_stats,
    temp: f64,

    tick: i64,
    last_tick: i64,

    polling_interval: u64,

    net_interface_name: String,
    hwmon_name: String,
    hwmon_sensor_name: String
}

impl Drop for Sysinfo {
    fn drop(&mut self) {
        unsafe { sg_shutdown(); }
    }
}

impl Sysinfo {
    pub fn render(&mut self) -> Result<FrameBuffer> {
        self.poll();

        let load = 100.0 - self.cpu.idle;
        let freq = get_cpufreq()? / 1000.0;
        let mem_used = self.mem.used as f64 / pow(1024, 3) as f64;
        let net_direction = if self.net.rx > self.net.tx {"I"} else {"O"};
        let (net_load, net_load_power, net_load_unit) = self.calculate_max_net_rate();
        let mut adjusted_net_load = format!("{:.4}", (net_load / 1024_f64.pow(net_load_power)).to_string());

        if adjusted_net_load.ends_with(".") {
            adjusted_net_load = adjusted_net_load.replace(".", "");
        }

        let mut buffer = FrameBuffer::new();

        self.render_stat(0, &mut buffer, format!("C: {:>4.0}%", load), load / 100.0)?;
        self.render_stat(1, &mut buffer, format!("F: {:>4.2}G", freq), freq / 7.0)?;
        self.render_stat(2, &mut buffer, format!("M: {:>4.1}G", mem_used), self.mem.used as f64 / self.mem.total as f64)?;
        self.render_stat(3, &mut buffer, format!("{}: {:>4}{}", net_direction, adjusted_net_load, net_load_unit), net_load / (100.0 * 1024_f64.pow(2)))?;
        self.render_stat(4, &mut buffer, format!("T: {:>4.1}C", self.temp), self.temp / 100.0)?;

        Ok(buffer)
    }

    fn calculate_max_net_rate(&self) -> (f64, i32, &str) {
        let max_diff = std::cmp::max(self.net.rx, self.net.tx) as f64;
        let max_rate = max_diff / ((self.tick - self.last_tick) as f64 / 1000.0);

        match max_rate {
            r if r > 1024_f64.pow(3) => (r, 3, "G"),
            r if r > 1024_f64.pow(2) => (r, 2, "M"),
            r if r > 1024_f64.pow(1) => (r, 1, "k"),
            r => (r, 0, "B")
        }
    }

    fn poll(&mut self) {
        self.temp = get_hwmon_temp(&self.hwmon_name, &self.hwmon_sensor_name);

        unsafe {
            let null = std::ptr::null_mut();
            let mut num_ifaces : usize = 0;

            sg_get_cpu_stats_diff(null);
            self.cpu = *sg_get_cpu_percents_of(sg_cpu_percent_source_sg_last_diff_cpu_percent, null);
            self.mem = *sg_get_mem_stats(null);

            let ifaces_ptr = sg_get_network_io_stats_diff(&mut num_ifaces);
            let ifaces = std::slice::from_raw_parts(ifaces_ptr, num_ifaces);

            self.net = *ifaces.iter().find(|iface| {
                let name = std::ffi::CStr::from_ptr(iface.interface_name);

                name.to_str().expect("could not retrieve name of network interface!")  == self.net_interface_name
            }).expect("could not find network interface!");

            sg_get_network_io_stats(null);
            self.last_tick = self.tick;
            self.tick = tick();
        }
    }

    fn render_stat(&self, slot: i32, buffer: &mut FrameBuffer, text : String, fill : f64) -> Result<()> {
        let style = MonoTextStyle::new(&ascii::FONT_4X6, BinaryColor::On);
        let metrics = style.measure_string(&text, Point::zero(), Baseline::Top);

        let slot_y = slot*8 + 1;

        Text::with_baseline(
            &text,
            Point::new(0, slot_y),
            style,
            Baseline::Top,
        )
        .draw(buffer)?;

        let bar_start: i32 = metrics.bounding_box.size.width as i32 + 2;
        let border_style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
        let fill_style = PrimitiveStyle::with_fill(BinaryColor::On);
        let fill_width = (fill * (127 - bar_start) as f64).floor() as i32;

        Rectangle::with_corners(
            Point::new(bar_start, slot_y),
            Point::new(127, slot_y + 6)
        ).into_styled(border_style)
         .draw(buffer)?;

        Rectangle::with_corners(
            Point::new(bar_start + 1, slot_y + 1),
            Point::new(bar_start + fill_width, slot_y + 5)
        ).into_styled(fill_style)
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
