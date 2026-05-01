use crate::{
    render::{display::ContentProvider, scheduler::ContentWrapper},
    scheduler::CONTENT_PROVIDERS,
};
use anyhow::{anyhow, Context, Result};
use apex_hardware::FrameBuffer;
use async_stream::try_stream;
use config::Config;
use embedded_graphics::{
    geometry::Point,
    pixelcolor::BinaryColor,
    primitives::{Primitive, PrimitiveStyle, Rectangle},
    Drawable,
};
use futures::Stream;
use linkme::distributed_slice;
use log::{info, warn};
use pipewire as pw;
use pw::{properties::properties, spa};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::os::unix::net::UnixStream; //Pipewire detection
use std::{
    convert::TryInto,
    env, //Pipewire detection
    mem::size_of,
    path::PathBuf, //Pipewire detection
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tokio::{time, time::MissedTickBehavior}; //Pipewire detection

const FFT_SIZE: usize = 2048; //FFT : Fast Fourier Transform
const DEFAULT_SAMPLE_RATE: u32 = 48_000;
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
    target_object: Option<String>,
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
    // fail early if PipeWire is not available
    ensure_pipewire_available()?;

    let min_frequency = config
        .get_float("equalizer.min_frequency")
        .unwrap_or(20.0)
        .clamp(20.0, 20_000.0) as f32;

    let min_db = config.get_float("equalizer.min_db").unwrap_or(-72.0) as f32;

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
        target_object: config.get_str("equalizer.target_object").ok(),
    };

    // shared state between capture and render
    let shared = SharedSpectrum::new(config.bar_count);
    start_capture(shared.clone(), config.clone())?;

    Ok(Box::new(Equalizer {
        polling_interval: config.polling_interval,
        bar_count: config.bar_count,
        shared: shared,
    }))
}
fn ensure_pipewire_available() -> Result<()> {
    // PipeWire runtime socket lives under XDG_RUNTIME_DIR
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| anyhow!("PipeWire equalizer needs a running PipeWire session"))?;
    let socket_path = PathBuf::from(runtime_dir).join("pipewire-0");

    // a socket connect is enough to check the session
    UnixStream::connect(&socket_path).map_err(|_| {
        anyhow!(
            "PipeWire equalizer needs a running PipeWire session (missing {})",
            socket_path.display()
        )
    })?;

    Ok(())
}

fn start_capture(shared: Arc<SharedSpectrum>, config: EqualizerConfig) -> Result<()> {
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    thread::Builder::new()
        .name("apex-tux-equalizer".to_string())
        .spawn(move || {
            // keep PipeWire on its own thread so audio work does not block rendering
            if let Err(error) = run_capture_thread(shared, config, ready_tx.clone()) {
                let message = error.to_string();
                let _ = ready_tx.send(Err(message.clone()));
                warn!("Equalizer capture stopped: {message}");
            }
        })
        .context("Failed to spawn the PipeWire equalizer thread")?;

    match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(())) => Ok(()),
        // bubble up capture errors
        Ok(Err(message)) => Err(anyhow!(message)),
        // don't hang forever on startup
        Err(mpsc::RecvTimeoutError::Timeout) => Err(anyhow!(
            "Timed out after {}s while waiting for PipeWire audio capture to start",
            READY_TIMEOUT.as_secs()
        )),
        // thread died
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!(
            "PipeWire equalizer thread exited before initialization"
        )),
    }
}

fn run_capture_thread(
    shared: Arc<SharedSpectrum>,
    config: EqualizerConfig,
    ready_tx: mpsc::SyncSender<Result<(), String>>,
) -> Result<()> {
    // PipeWire wants global init first
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    // tell PipeWire this is audio capture
    let mut props = properties! {
        *pw::keys::APP_NAME => "apex-tux",
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "DSP",
        *pw::keys::NODE_LATENCY => "1024/48000",
    };

    if config.capture_sink {
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }
    if let Some(target_object) = config.target_object.as_deref() {
        // allow routing to a specific node
        props.insert("target.object", target_object);
    }

    // this stream feeds the FFT pipeline
    let stream = pw::stream::StreamBox::new(&core, "apex-tux-equalizer", props)?;
    let _listener = stream
        .add_local_listener_with_user_data(CaptureState::new(shared, &config))
        .process(|stream, user_data| {
            // no buffer, no work
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }

            let data = &mut datas[0];
            // ignore empty chunks
            if data.chunk().size() == 0 {
                return;
            }

            // interleaved f32 audio
            let channels = user_data.format.channels().max(1) as usize;
            let sample_size = size_of::<f32>();
            let frame_bytes = channels.saturating_mul(sample_size);
            let chunk_size = data.chunk().size() as usize;
            if frame_bytes == 0 {
                return;
            }

            if let Some(bytes) = data.data() {
                // trim to whole frames
                let byte_count = chunk_size.min(bytes.len());
                let aligned_count = byte_count - (byte_count % frame_bytes);
                if aligned_count == 0 {
                    return;
                }
                // collapse frames to mono before the FFT
                user_data.push_samples(&bytes[..aligned_count]);
                user_data.analyze();
            }
        })
        .param_changed(|_, user_data, id, param| {
            // only care about format changes
            let Some(param) = param else {
                return;
            };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }

            // parse the PipeWire format payload
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };

            if media_type != spa::param::format::MediaType::Audio
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            // rebuild bands when the rate changes
            if user_data.format.parse(param).is_ok() {
                user_data.rebuild_bands(user_data.format.rate());
            }
        })
        .register()?;

    // build the requested sample format
    let values = build_capture_params()?;
    let pod = spa::pod::Pod::from_bytes(&values).context("Failed to build PipeWire pod")?;
    let mut params = [pod];

    // connect as an input stream
    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    // signal readiness once connected
    ready_tx
        .send(Ok(()))
        .map_err(|_| anyhow!("Failed to report PipeWire equalizer readiness"))?;

    // keep the thread alive while the stream runs
    mainloop.run();
    Ok(())
}

fn build_capture_params() -> Result<Vec<u8>> {
    // request 32-bit float PCM
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);

    // wrap the audio format in a PipeWire object
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: audio_info.into(),
    };

    // serialize the pod payload
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .context("Failed to serialize PipeWire audio format")?
    .0
    .into_inner();

    Ok(values)
}

struct CaptureState {
    shared: Arc<SharedSpectrum>,
    format: spa::param::audio::AudioInfoRaw,
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
    fn new(shared: Arc<SharedSpectrum>, config: &EqualizerConfig) -> Self {
        // preallocate FFT state
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let spectrum = vec![Complex::default(); FFT_SIZE];
        // cosine window to cut leakage
        let window = (0..FFT_SIZE)
            .map(|index| {
                let phase = (2.0 * std::f32::consts::PI * index as f32) / (FFT_SIZE as f32 - 1.0);
                phase.cos()
            })
            .collect::<Vec<_>>();

        let mut state = Self {
            shared,
            format: Default::default(),
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

        // build the initial band layout
        state.rebuild_bands(DEFAULT_SAMPLE_RATE);
        state
    }

    fn rebuild_bands(&mut self, sample_rate: u32) {
        // clamp the sample rate
        let sample_rate = sample_rate.max(1);
        let nyquist = sample_rate as f32 / 2.0;
        let min_hz = self.min_frequency.min(nyquist - 1.0).max(20.0);
        let max_hz = self.max_frequency.min(nyquist).max(min_hz + 1.0);
        let max_bin = FFT_SIZE / 2;
        let band_count = self.band_ranges.len();

        for (index, band) in self.band_ranges.iter_mut().enumerate() {
            // spread bands exponentially
            let start_hz =
                exponential_interpolate(min_hz, max_hz, index as f32 / band_count as f32);
            let end_hz =
                exponential_interpolate(min_hz, max_hz, (index + 1) as f32 / band_count as f32);

            // convert the span into FFT bins
            let start_bin = hz_to_bin(start_hz, sample_rate).clamp(1, max_bin);
            let end_bin = if index + 1 == band_count {
                // let the last band reach the top
                max_bin + 1
            } else {
                hz_to_bin(end_hz, sample_rate).clamp(start_bin + 1, max_bin + 1)
            };

            *band = (start_bin, end_bin);
        }
    }

    fn push_samples(&mut self, bytes: &[u8]) {
        // frames are little-endian f32 samples
        let channels = self.format.channels().max(1) as usize;
        let frame_bytes = channels * size_of::<f32>();

        for frame in bytes.chunks_exact(frame_bytes) {
            let mut mono = 0.0;
            for sample in frame.chunks_exact(size_of::<f32>()) {
                mono += f32::from_le_bytes(sample.try_into().expect("invalid sample width"));
            }
            // average channels down to mono
            self.samples.push(mono / channels as f32);
        }

        // keep a bounded sample history
        let max_history = FFT_SIZE * 4;
        if self.samples.len() > max_history {
            let overflow = self.samples.len() - max_history;
            self.samples.drain(..overflow);
        }
    }

    fn analyze(&mut self) {
        // wait for a full window
        if self.samples.len() < FFT_SIZE {
            return;
        }

        // copy the latest block into the FFT input
        let start = self.samples.len() - FFT_SIZE;
        for (slot, sample) in self.spectrum.iter_mut().zip(&self.samples[start..]) {
            slot.re = *sample;
            slot.im = 0.0;
        }

        // apply the window
        for (slot, window) in self.spectrum.iter_mut().zip(&self.window) {
            slot.re *= *window;
        }

        // run the FFT in place
        self.fft
            .process_with_scratch(&mut self.spectrum, &mut self.scratch);

        let scale = 1.0 / FFT_SIZE as f32;
        let db_span = (self.max_db - self.min_db).max(1.0);

        for (index, (start_bin, end_bin)) in self.band_ranges.iter().copied().enumerate() {
            // track the strongest bin in each band
            let mut peak = 1.0e-6_f32;
            for bin in start_bin..end_bin {
                peak = peak.max(self.spectrum[bin].norm() * scale);
            }

            // map magnitude to a display range
            let db = 20.0 * peak.log10();
            let normalized = ((db - self.min_db) / db_span).clamp(0.0, 1.0);
            let factor = if normalized > self.smoothed[index] {
                // faster on the way up
                self.rise_smoothing
            } else {
                // slower on the way down
                self.fall_smoothing
            };
            // smooth the bar motion
            self.smoothed[index] += (normalized - self.smoothed[index]) * factor;

            // publish the 8-bit bar height
            self.shared.bars[index].store(
                (self.smoothed[index] * f32::from(u8::MAX)).round() as u8,
                Ordering::Relaxed,
            );
        }

        // mark the spectrum active
        self.shared.active.store(true, Ordering::Relaxed);
    }
}

fn exponential_interpolate(min_hz: f32, max_hz: f32, t: f32) -> f32 {
    // logarithmic spacing favors low frequencies
    min_hz * (max_hz / min_hz).powf(t)
}

fn hz_to_bin(hz: f32, sample_rate: u32) -> usize {
    // convert a frequency to an FFT bin
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
        // keep the display blank until data arrives
        if !self.shared.active.load(Ordering::Relaxed) {
            return Ok(buffer);
        }

        // use a small gap when it fits
        let gap = usize::from(self.bar_count <= 64);
        let usable_width = DISPLAY_WIDTH.saturating_sub(gap * self.bar_count.saturating_sub(1));
        let bar_width = (usable_width / self.bar_count).max(1);
        let used_width = bar_width * self.bar_count + gap * self.bar_count.saturating_sub(1);
        let left_padding = ((DISPLAY_WIDTH.saturating_sub(used_width)) / 2) as i32;
        let style = PrimitiveStyle::with_fill(BinaryColor::On);

        for (index, value) in self.shared.bars.iter().enumerate() {
            // shared values are 0..255 heights
            let normalized = f32::from(value.load(Ordering::Relaxed)) / f32::from(u8::MAX);
            let height = (normalized * DISPLAY_HEIGHT as f32).round() as i32;
            if height <= 0 {
                continue;
            }

            // turn the height into a rectangle
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
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    fn stream(&mut self) -> Result<<Self as ContentProvider>::ContentStream<'_>> {
        // refresh as fast as the audio polling
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
