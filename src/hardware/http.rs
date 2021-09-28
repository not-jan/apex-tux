use crate::{hardware::device::Device, render::display::FrameBuffer};
use anyhow::{anyhow, Result};
use log::info;
use reqwest::{header, Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::{env, fs::File, path::PathBuf};

#[derive(Debug, Clone)]
pub struct SteelseriesEngine {
    address: String,
    client: Client,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CoreProps {
    address: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct HandleRegistration {
    game: &'static str,
    game_display_name: &'static str,
    developer: &'static str,
}

impl Default for HandleRegistration {
    fn default() -> Self {
        Self {
            game: env!("CARGO_PKG_NAME"),
            game_display_name: env!("CARGO_PKG_NAME"),
            developer: "not-jan",
        }
    }
}

impl SteelseriesEngine {
    pub async fn try_connect() -> Result<Self> {
        let program_data = env::var("PROGRAMDATA")?;

        let mut buf = PathBuf::from(program_data);
        buf.push("SteelSeries");
        buf.push("SteelSeries Engine 3");
        buf.push("coreProps.json");

        info!(
            "Trying to read coreProps.json from `{}`",
            buf.to_string_lossy()
        );

        let file = File::open(&buf)?;

        let props: CoreProps = serde_json::from_reader(&file)?;

        info!("SteelSeries server: `{}`", &props.address);

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        let client = ClientBuilder::new().default_headers(headers).build()?;

        let registration_url = format!("http://{}/game_metadata", &props.address);
        let payload = HandleRegistration::default();

        let result = client.post(&registration_url).json(&payload).send().await?;

        info!(
            "Received {} from the SteelSeries engine",
            result.text().await?
        );

        Ok(Self {
            address: props.address,
            client,
        })
    }
}

impl Device for SteelseriesEngine {
    fn draw(&mut self, display: &FrameBuffer) -> Result<()> {
        todo!()
    }

    fn clear(&mut self) -> Result<()> {
        let clear = FrameBuffer::new();
        self.draw(&clear)
    }
}
