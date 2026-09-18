use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::{anyhow, Context, Result};
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use config::Config;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, Sample, SampleFormat, SizedSample, Stream, SupportedStreamConfig,
};
use embedded_graphics::{
    geometry::Point,
    pixelcolor::BinaryColor,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    Drawable,
};
use futures::Stream as FuturesStream;
use linkme::distributed_slice;
use log::{info, warn};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tokio::{time, time::MissedTickBehavior};

const FFT_SIZE: usize = 2048;
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const DISPLAY_WIDTH: usize = 128;
const DISPLAY_HEIGHT: i32 = 40;

#[doc(hidden)]
#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[derive(Debug, Clone)]
struct EqualizerConfig {
    polling_interval: u64,
    bar_count: usize,
    min_frequency: f32,
    max_frequency: f32,
    min_db: f32,
    max_db: f32,
    rise_smoothing: f32,
    fall_smoothing: f32,
    capture_sink: bool,
    device_name: Option<String>,
}

struct CaptureDeviceSelection {
    device: Device,
    supported_config: SupportedStreamConfig,
    device_name: String,
    loopback: bool,
}

#[derive(Debug)]
struct SharedSpectrum {
    bars: Arc<[AtomicU8]>,
    active: AtomicBool,
}

impl SharedSpectrum {
    fn new(bar_count: usize) -> Arc<Self> {
        let bars = (0..bar_count)
            .map(|_| AtomicU8::new(0))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Arc::new(Self {
            bars: Arc::from(bars),
            active: AtomicBool::new(false),
        })
    }
}

#[doc(hidden)]
#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Equalizer display source.");

    let min_frequency = config
        .get_float("equalizer.min_frequency")
        .unwrap_or(20.0)
        .clamp(20.0, 20_000.0) as f32;

    let min_db = config.get_float("equalizer.min_db").unwrap_or(-72.0) as f32;

    let device_name = config
        .get_str("equalizer.device_name")
        .ok()
        .or_else(|| config.get_str("equalizer.target_object").ok());

    if config.get_bool("equalizer.capture_sink").unwrap_or(true) {
        info!(
            "CPAL equalizer depends on the OS exposing a loopback-capable input device; choose \
             one with equalizer.device_name when needed."
        );
    }

    let config = EqualizerConfig {
        polling_interval: config
            .get_int("equalizer.polling_interval")
            .unwrap_or(20)
            .clamp(16, 1000) as u64,
        bar_count: config
            .get_int("equalizer.bar_count")
            .unwrap_or(32)
            .clamp(4, 128) as usize,
        min_frequency,
        max_frequency: config
            .get_float("equalizer.max_frequency")
            .unwrap_or(16_000.0)
            .max(f64::from(min_frequency) + 1.0)
            .min(20_000.0) as f32,
        min_db,
        max_db: config
            .get_float("equalizer.max_db")
            .unwrap_or(-18.0)
            .max(f64::from(min_db) + 1.0) as f32,
        rise_smoothing: config
            .get_float("equalizer.rise_smoothing")
            .unwrap_or(0.6)
            .clamp(0.01, 1.0) as f32,
        fall_smoothing: config
            .get_float("equalizer.fall_smoothing")
            .unwrap_or(0.4)
            .clamp(0.01, 1.0) as f32,
        capture_sink: config.get_bool("equalizer.capture_sink").unwrap_or(true),
        device_name,
    };

    let shared = SharedSpectrum::new(config.bar_count);
    start_capture(shared.clone(), config.clone())?;

    Ok(Box::new(Equalizer {
        polling_interval: config.polling_interval,
        bar_count: config.bar_count,
        shared,
    }))
}

fn start_capture(shared: Arc<SharedSpectrum>, config: EqualizerConfig) -> Result<()> {
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    thread::Builder::new()
        .name("apex-tux-equalizer".to_string())
        .spawn(move || {
            if let Err(error) = run_capture_thread(shared, config, ready_tx.clone()) {
                let message = error.to_string();
                let _ = ready_tx.send(Err(message.clone()));
                warn!("Equalizer capture stopped: {message}");
            }
        })
        .context("Failed to spawn the CPAL equalizer thread")?;

    match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(message)) => Err(anyhow!(message)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "Timed out after {}s while waiting for CPAL audio capture to start",
            READY_TIMEOUT.as_secs()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!(
            "CPAL equalizer thread exited before initialization"
        )),
    }
}

fn run_capture_thread(
    shared: Arc<SharedSpectrum>,
    config: EqualizerConfig,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
) -> Result<()> {
    let selection = select_capture_device(&config)?;
    let stream_config = selection.supported_config.config();

    info!(
        "Using CPAL equalizer {} device: {} ({} ch @ {} Hz, {:?})",
        if selection.loopback {
            "loopback"
        } else {
            "input"
        },
        selection.device_name,
        stream_config.channels,
        stream_config.sample_rate.0,
        selection.supported_config.sample_format()
    );

    let state = CaptureState::new(
        shared,
        &config,
        stream_config.sample_rate.0,
        stream_config.channels as usize,
    );
    let stream = build_input_stream(
        &selection.device,
        &selection.supported_config,
        state,
        &selection.device_name,
        selection.loopback,
    )?;
    stream
        .play()
        .context("Failed to start the CPAL equalizer input stream")?;

    ready_tx
        .send(Ok(()))
        .map_err(|_| anyhow!("Failed to report CPAL equalizer readiness"))?;

    loop {
        let _keep_stream_alive = &stream;
        thread::park_timeout(Duration::from_secs(60));
    }
}

fn select_capture_device(config: &EqualizerConfig) -> Result<CaptureDeviceSelection> {
    #[cfg(target_os = "windows")]
    if config.capture_sink {
        return select_output_loopback_device(config.device_name.as_deref());
    }

    select_input_device(config.device_name.as_deref())
}

fn select_input_device(preferred_name: Option<&str>) -> Result<CaptureDeviceSelection> {
    let host = cpal::default_host();

    if let Some(preferred_name) = preferred_name {
        let needle = preferred_name.trim().to_ascii_lowercase();
        let mut available = Vec::new();

        for device in host.devices().context("Failed to enumerate CPAL devices")? {
            if !device_supports_input(&device) {
                continue;
            }

            let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
            available.push(device_name.clone());

            if device_name.eq_ignore_ascii_case(preferred_name)
                || device_name.to_ascii_lowercase().contains(&needle)
            {
                return build_input_selection(device);
            }
        }

        return Err(anyhow!(
            "No CPAL input device matched {:?}. Available input devices: \n\t{}",
            preferred_name,
            if available.is_empty() {
                "<none>".to_string()
            } else {
                available.join("\n\t ")
            }
        ));
    }

    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("No default CPAL input device available"))?;
    build_input_selection(device)
}

fn device_supports_input(device: &Device) -> bool {
    if device.default_input_config().is_ok() {
        return true;
    }

    device
        .supported_input_configs()
        .map(|mut configs| configs.next().is_some())
        .unwrap_or(false)
}

fn build_input_selection(device: Device) -> Result<CaptureDeviceSelection> {
    let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
    let supported_config = select_input_config(&device)?;
    Ok(CaptureDeviceSelection {
        device,
        supported_config,
        device_name,
        loopback: false,
    })
}

fn select_input_config(device: &Device) -> Result<SupportedStreamConfig> {
    if let Ok(config) = device.default_input_config() {
        return Ok(config);
    }

    device
        .supported_input_configs()
        .context("Failed to query supported input configs")?
        .next()
        .map(|config| config.with_max_sample_rate())
        .ok_or_else(|| anyhow!("The selected CPAL input device has no supported input configs"))
}

#[cfg(target_os = "windows")]
fn select_output_loopback_device(preferred_name: Option<&str>) -> Result<CaptureDeviceSelection> {
    let host = cpal::default_host();

    if let Some(preferred_name) = preferred_name {
        let needle = preferred_name.trim().to_ascii_lowercase();
        let mut available = Vec::new();

        for device in host.devices().context("Failed to enumerate CPAL devices")? {
            if !device_supports_output(&device) {
                continue;
            }

            let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
            available.push(device_name.clone());

            if device_name.eq_ignore_ascii_case(preferred_name)
                || device_name.to_ascii_lowercase().contains(&needle)
            {
                return build_output_loopback_selection(device);
            }
        }

        return Err(anyhow!(
            "No CPAL output device matched {:?} for loopback capture. Available output devices: \
             \n\t{}",
            preferred_name,
            if available.is_empty() {
                "<none>".to_string()
            } else {
                available.join("\n\t ")
            }
        ));
    }

    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("No default CPAL output device available for loopback capture"))?;
    build_output_loopback_selection(device)
}

#[cfg(target_os = "windows")]
fn device_supports_output(device: &Device) -> bool {
    if device.default_output_config().is_ok() {
        return true;
    }

    device
        .supported_output_configs()
        .map(|mut configs| configs.next().is_some())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn build_output_loopback_selection(device: Device) -> Result<CaptureDeviceSelection> {
    let device_name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
    let supported_config = select_output_config(&device)?;
    Ok(CaptureDeviceSelection {
        device,
        supported_config,
        device_name,
        loopback: true,
    })
}

#[cfg(target_os = "windows")]
fn select_output_config(device: &Device) -> Result<SupportedStreamConfig> {
    if let Ok(config) = device.default_output_config() {
        return Ok(config);
    }

    device
        .supported_output_configs()
        .context("Failed to query supported output configs")?
        .next()
        .map(|config| config.with_max_sample_rate())
        .ok_or_else(|| anyhow!("The selected CPAL output device has no supported output configs"))
}

fn build_input_stream(
    device: &Device,
    supported_config: &SupportedStreamConfig,
    state: CaptureState,
    device_name: &str,
    loopback: bool,
) -> Result<Stream> {
    let stream_config = supported_config.config();
    let capture_mode = if loopback { "loopback" } else { "input" };

    match supported_config.sample_format() {
        SampleFormat::F32 => build_typed_input_stream::<f32>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::F64 => build_typed_input_stream::<f64>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::I8 => {
            build_typed_input_stream::<i8>(device, &stream_config, state, device_name, capture_mode)
        }
        SampleFormat::I16 => build_typed_input_stream::<i16>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::I32 => build_typed_input_stream::<i32>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::I64 => build_typed_input_stream::<i64>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::U8 => {
            build_typed_input_stream::<u8>(device, &stream_config, state, device_name, capture_mode)
        }
        SampleFormat::U16 => build_typed_input_stream::<u16>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::U32 => build_typed_input_stream::<u32>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        SampleFormat::U64 => build_typed_input_stream::<u64>(
            device,
            &stream_config,
            state,
            device_name,
            capture_mode,
        ),
        other => Err(anyhow!(
            "Unsupported CPAL input sample format for equalizer: {other:?}"
        )),
    }
}

fn build_typed_input_stream<T>(
    device: &Device,
    config: &cpal::StreamConfig,
    mut state: CaptureState,
    device_name: &str,
    capture_mode: &str,
) -> Result<Stream>
where
    T: Sample + SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                state.push_samples(data);
                state.analyze();
            },
            handle_stream_error,
            None,
        )
        .map_err(|error| {
            let detail = format_build_stream_error(&error);
            anyhow!(
                "Failed to build CPAL {capture_mode} stream for device {:?} with config {} ch @ \
                 {} Hz: {}",
                device_name,
                config.channels,
                config.sample_rate.0,
                detail
            )
        })
}

fn handle_stream_error(error: cpal::StreamError) {
    warn!("Equalizer audio stream error: {error}");
}

fn format_build_stream_error(error: &cpal::BuildStreamError) -> String {
    let raw = error.to_string();

    #[cfg(target_os = "windows")]
    if raw.contains("0x8889000A") {
        return format!(
            "{raw} (the Windows audio endpoint appears to be in exclusive use; disable the \
             device's exclusive-control setting or close the app currently holding it, then try \
             again. See the README Windows audio notes.)"
        );
    }

    raw
}

struct CaptureState {
    shared: Arc<SharedSpectrum>,
    channels: usize,
    fft: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    spectrum: Vec<Complex<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    band_ranges: Vec<(usize, usize)>,
    smoothed: Vec<f32>,
    min_frequency: f32,
    max_frequency: f32,
    min_db: f32,
    max_db: f32,
    rise_smoothing: f32,
    fall_smoothing: f32,
}

impl CaptureState {
    fn new(
        shared: Arc<SharedSpectrum>,
        config: &EqualizerConfig,
        sample_rate: u32,
        channels: usize,
    ) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let spectrum = vec![Complex::default(); FFT_SIZE];
        let window = (0..FFT_SIZE)
            .map(|index| {
                let phase = (2.0 * std::f32::consts::PI * index as f32) / (FFT_SIZE as f32 - 1.0);
                phase.cos()
            })
            .collect::<Vec<_>>();

        let mut state = Self {
            shared,
            channels: channels.max(1),
            fft,
            scratch,
            spectrum,
            window,
            samples: Vec::with_capacity(FFT_SIZE * 4),
            band_ranges: vec![(1, 2); config.bar_count],
            smoothed: vec![0.0; config.bar_count],
            min_frequency: config.min_frequency,
            max_frequency: config.max_frequency,
            min_db: config.min_db,
            max_db: config.max_db,
            rise_smoothing: config.rise_smoothing,
            fall_smoothing: config.fall_smoothing,
        };

        state.rebuild_bands(sample_rate);
        state
    }

    fn rebuild_bands(&mut self, sample_rate: u32) {
        let sample_rate = sample_rate.max(1);
        let nyquist = sample_rate as f32 / 2.0;
        let min_hz = self.min_frequency.min(nyquist - 1.0).max(20.0);
        let max_hz = self.max_frequency.min(nyquist).max(min_hz + 1.0);
        let max_bin = FFT_SIZE / 2;
        let band_count = self.band_ranges.len();

        for (index, band) in self.band_ranges.iter_mut().enumerate() {
            let start_hz =
                exponential_interpolate(min_hz, max_hz, index as f32 / band_count as f32);
            let end_hz =
                exponential_interpolate(min_hz, max_hz, (index + 1) as f32 / band_count as f32);

            let start_bin = hz_to_bin(start_hz, sample_rate).clamp(1, max_bin);
            let end_bin = if index + 1 == band_count {
                max_bin + 1
            } else {
                hz_to_bin(end_hz, sample_rate).clamp(start_bin + 1, max_bin + 1)
            };

            *band = (start_bin, end_bin);
        }
    }

    fn push_samples<T>(&mut self, data: &[T])
    where
        T: Sample,
        f32: FromSample<T>,
    {
        for frame in data.chunks_exact(self.channels) {
            let mut mono = 0.0;
            for sample in frame {
                mono += sample.to_sample::<f32>();
            }
            self.samples.push(mono / self.channels as f32);
        }

        let max_history = FFT_SIZE * 4;
        if self.samples.len() > max_history {
            let overflow = self.samples.len() - max_history;
            self.samples.drain(..overflow);
        }
    }

    fn analyze(&mut self) {
        if self.samples.len() < FFT_SIZE {
            return;
        }

        let start = self.samples.len() - FFT_SIZE;
        for (slot, sample) in self.spectrum.iter_mut().zip(&self.samples[start..]) {
            slot.re = *sample;
            slot.im = 0.0;
        }

        for (slot, window) in self.spectrum.iter_mut().zip(&self.window) {
            slot.re *= *window;
        }

        self.fft
            .process_with_scratch(&mut self.spectrum, &mut self.scratch);

        let scale = 1.0 / FFT_SIZE as f32;
        let db_span = (self.max_db - self.min_db).max(1.0);

        for (index, (start_bin, end_bin)) in self.band_ranges.iter().copied().enumerate() {
            let mut peak = 1.0e-6_f32;
            for bin in start_bin..end_bin {
                peak = peak.max(self.spectrum[bin].norm() * scale);
            }

            let db = 20.0 * peak.log10();
            let normalized = ((db - self.min_db) / db_span).clamp(0.0, 1.0);
            let factor = if normalized > self.smoothed[index] {
                self.rise_smoothing
            } else {
                self.fall_smoothing
            };
            self.smoothed[index] += (normalized - self.smoothed[index]) * factor;

            self.shared.bars[index].store(
                (self.smoothed[index] * f32::from(u8::MAX)).round() as u8,
                Ordering::Relaxed,
            );
        }

        self.shared.active.store(true, Ordering::Relaxed);
    }
}

fn exponential_interpolate(min_hz: f32, max_hz: f32, t: f32) -> f32 {
    min_hz * (max_hz / min_hz).powf(t)
}

fn hz_to_bin(hz: f32, sample_rate: u32) -> usize {
    ((hz / sample_rate as f32) * FFT_SIZE as f32).round() as usize
}

pub struct Equalizer {
    polling_interval: u64,
    bar_count: usize,
    shared: Arc<SharedSpectrum>,
}

impl Equalizer {
    fn render(&self) -> Result<FrameBuffer> {
        let mut buffer = FrameBuffer::new();
        if !self.shared.active.load(Ordering::Relaxed) {
            return Ok(buffer);
        }

        let gap = usize::from(self.bar_count <= 64);
        let usable_width = DISPLAY_WIDTH.saturating_sub(gap * self.bar_count.saturating_sub(1));
        let bar_width = (usable_width / self.bar_count).max(1);
        let used_width = bar_width * self.bar_count + gap * self.bar_count.saturating_sub(1);
        let left_padding = ((DISPLAY_WIDTH.saturating_sub(used_width)) / 2) as i32;
        let style = PrimitiveStyle::with_fill(BinaryColor::On);

        for (index, value) in self.shared.bars.iter().enumerate() {
            let normalized = f32::from(value.load(Ordering::Relaxed)) / f32::from(u8::MAX);
            let height = (normalized * DISPLAY_HEIGHT as f32).round() as i32;
            if height <= 0 {
                continue;
            }

            let x = left_padding + index as i32 * (bar_width + gap) as i32;
            let top = DISPLAY_HEIGHT - height;
            Rectangle::with_corners(
                Point::new(x, top),
                Point::new(x + bar_width as i32 - 1, DISPLAY_HEIGHT - 1),
            )
            .into_styled(style)
            .draw(&mut buffer)?;
        }

        Ok(buffer)
    }
}

impl ContentProvider for Equalizer {
    type ContentStream<'a> = impl FuturesStream<Item = Result<FrameBuffer>> + 'a;

    fn stream(&mut self) -> Result<<Self as ContentProvider>::ContentStream<'_>> {
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
        "equalizer"
    }
}
