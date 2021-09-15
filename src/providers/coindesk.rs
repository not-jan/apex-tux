use crate::render::{
    display::{ContentProvider, FrameBuffer},
    scheduler::{ContentWrapper, CONTENT_PROVIDERS},
};
use anyhow::Result;
use async_rwlock::RwLock;
use async_stream::try_stream;
use embedded_graphics::{
    geometry::{OriginDimensions, Point},
    image::Image,
    mono_font::{ascii, MonoTextStyle},
    pixelcolor::BinaryColor,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
};
use futures::Stream;
use linkme::distributed_slice;
use log::info;
use reqwest::{header, Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::{lazy::SyncLazy, time::Duration};
use tinybmp::Bmp;
use tokio::{time, time::MissedTickBehavior};

static BTC_ICON: &[u8] = include_bytes!("./../../assets/btc.bmp");

static BTC_BMP: SyncLazy<Bmp<BinaryColor>> = SyncLazy::new(|| {
    Bmp::<BinaryColor>::from_slice(BTC_ICON).expect("Failed to parse BMP for BTC icon!")
});

#[distributed_slice(CONTENT_PROVIDERS)]
static PROVIDER_INIT: fn() -> Result<Box<dyn ContentWrapper>> = register_callback;

#[allow(clippy::unnecessary_wraps)]
fn register_callback() -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Coindesk display source.");
    Ok(Box::new(Coindesk::new()?))
}

const COINDESK_URL: &str = "https://api.coindesk.com/v1/bpi/currentprice.json";

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Currency {
    code: String,
    symbol: String,
    rate: String,
    description: String,
    rate_float: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Time {
    updated: String,
    #[serde(rename(serialize = "updatedISO", deserialize = "updatedISO"))]
    updated_iso: String,
    updateduk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BitcoinPrice {
    #[serde(rename(serialize = "USD", deserialize = "USD"))]
    usd: Currency,
    #[serde(rename(serialize = "GBP", deserialize = "GBP"))]
    gbp: Currency,
    #[serde(rename(serialize = "EUR", deserialize = "EUR"))]
    eur: Currency,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Status {
    time: Time,
    disclaimer: String,
    #[serde(rename(serialize = "chartName", deserialize = "chartName"))]
    chart_name: String,
    bpi: BitcoinPrice,
}

impl Status {
    pub fn render(&self) -> Result<FrameBuffer> {
        let mut buffer = FrameBuffer::new();

        // TODO: Add support for EUR and GBP since we're fetching them anyway
        let text = format!(" ${}", self.bpi.usd.rate);
        let style = MonoTextStyle::new(&ascii::FONT_6X13_BOLD, BinaryColor::On);
        Image::new(
            &*BTC_BMP,
            Point::new(0, 40 / 2 - (BTC_BMP.size().height / 2) as i32),
        )
        .draw(&mut buffer)?;

        let metrics = style.measure_string(&text, Point::zero(), Baseline::Top);
        let height: i32 = (metrics.bounding_box.size.height / 2) as i32;
        Text::with_baseline(&text, Point::new(24, 40 / 2 - height), style, Baseline::Top)
            .draw(&mut buffer)?;
        Ok(buffer)
    }
}

#[derive(Debug, Clone, Default)]
struct Coindesk(Client);

impl From<Client> for Coindesk {
    fn from(inner: Client) -> Self {
        Self(inner)
    }
}

impl Coindesk {
    pub fn new() -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        Ok(Coindesk(
            ClientBuilder::new()
                .user_agent(APP_USER_AGENT)
                .default_headers(headers)
                .build()?,
        ))
    }

    pub async fn fetch(&self) -> Result<Status> {
        let status = self
            .0
            .get(COINDESK_URL)
            .send()
            .await?
            .json::<Status>()
            .await?;

        Ok(status)
    }
}

impl ContentProvider for Coindesk {
    type ContentStream<'a> = impl Stream<Item = Result<FrameBuffer>> + 'a;

    #[allow(clippy::needless_lifetimes)]
    fn stream<'this>(&'this mut self) -> Result<Self::ContentStream<'this>> {
        // Coindesk updates its data every minute so we only need to fetch every minute
        let mut refetch = time::interval(Duration::from_secs(60));
        refetch.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // The scheduler expect a new image every so often so if no image is delivered
        // it'll just display a black image until the refetch timer ran.
        let mut render = time::interval(Duration::from_millis(50));
        render.set_missed_tick_behavior(MissedTickBehavior::Skip);

        // We need some sort of synchronization between the task that displays the data
        // and the task that fetches it
        let status = RwLock::new(FrameBuffer::new());

        Ok(try_stream! {
            loop {
                tokio::select! {
                    _ = render.tick() => {
                        let buffer = status.read().await;
                        yield *buffer;
                    },
                    _ = refetch.tick() => {
                        let data = self.fetch().await.and_then(|d| d.render());
                        let mut buffer = status.write().await;
                        if let Ok(data) = data {
                            *buffer = data;
                        }
                    }
                }
            }
        })
    }

    fn name(&self) -> &'static str {
        "coindesk"
    }
}
