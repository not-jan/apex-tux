use crate::render::{
    display::ContentProvider,
    scheduler::{ContentWrapper, CONTENT_PROVIDERS},
};
use anyhow::{anyhow, Result};
use apex_hardware::FrameBuffer;
use async_rwlock::RwLock;
use async_stream::try_stream;
use config::Config;
use embedded_graphics::{
    geometry::{OriginDimensions, Point},
    image::Image,
    mono_font::{iso_8859_15, MonoTextStyle},
    pixelcolor::BinaryColor,
    text::{renderer::TextRenderer, Baseline, Text},
    Drawable,
};
use futures::Stream;
use lazy_static::lazy_static;
use linkme::distributed_slice;
use log::info;
use reqwest::{header, Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, time::Duration};
use tinybmp::Bmp;
use tokio::{time, time::MissedTickBehavior};

static BTC_ICON: &[u8] = include_bytes!("./../../assets/btc.bmp");

lazy_static! {
    static ref BTC_BMP: Bmp<'static, BinaryColor> =
        Bmp::<BinaryColor>::from_slice(BTC_ICON).expect("Failed to parse BMP for BTC icon!");
}

#[distributed_slice(CONTENT_PROVIDERS)]
pub static PROVIDER_INIT: fn(&Config) -> Result<Box<dyn ContentWrapper>> = register_callback;

#[derive(Debug, Copy, Clone)]
pub enum Target {
    Eur,
    Usd,
    Gbp,
}

impl Default for Target {
    fn default() -> Self {
        Target::Usd
    }
}

impl TryFrom<String> for Target {
    type Error = anyhow::Error;

    fn try_from(value: String) -> std::prelude::rust_2015::Result<Self, Self::Error> {
        match value.as_str() {
            "USD" | "usd" | "dollar" => Ok(Target::Usd),
            "eur" | "EUR" | "euro" | "Euro" => Ok(Target::Eur),
            "gbp" | "GBP" => Ok(Target::Gbp),
            _ => Err(anyhow!("Unknown target currency!")),
        }
    }
}

impl Target {
    pub fn format(self, price: &BitcoinPrice) -> String {
        match self {
            Target::Eur => format!("{}\u{20ac}", price.eur.rate),
            Target::Usd => format!("${}", price.usd.rate),
            Target::Gbp => format!("\u{a3}{}", price.gbp.rate),
        }
    }
}

#[allow(clippy::unnecessary_wraps)]
fn register_callback(config: &Config) -> Result<Box<dyn ContentWrapper>> {
    info!("Registering Coindesk display source.");
    let currency = config
        .get_str("crypto.currency")
        .unwrap_or_else(|_| String::from("USD"));
    let currency = Target::try_from(currency).unwrap_or_default();
    Ok(Box::new(Coindesk::new(currency)?))
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
    pub fn render(&self, target: Target) -> Result<FrameBuffer> {
        let mut buffer = FrameBuffer::new();

        // TODO: Add support for EUR and GBP since we're fetching them anyway
        let text = target.format(&self.bpi);
        let style = MonoTextStyle::new(&iso_8859_15::FONT_6X13_BOLD, BinaryColor::On);
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
struct Coindesk {
    client: Client,
    target: Target,
}

impl Coindesk {
    pub fn new(target: Target) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        Ok(Coindesk {
            client: ClientBuilder::new()
                .user_agent(APP_USER_AGENT)
                .default_headers(headers)
                .build()?,
            target,
        })
    }

    pub async fn fetch(&self) -> Result<Status> {
        let status = self
            .client
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
                        let data = self.fetch().await.and_then(|d| d.render(self.target));
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
