use anyhow::Result;
use apex_hardware::{AsyncDevice, FrameBuffer};
use gamesense::raw_client::{
    BindGameEvent, FrameContainer, GameEvent, Heartbeat, RawGameSenseClient, RegisterGame,
    RemoveEvent, RemoveGame, Screen, ScreenFrameData, ScreenHandler, Sendable,
};
use std::future::Future;

use log::info;
const GAME: &str = "APEXTUX";
const EVENT: &str = "SCREEN";

const REGISTER_GAME: RegisterGame = RegisterGame {
    game: GAME,
    display_name: Some("apex-tux"),
    developer: Some("not-jan"),
    timeout: None,
};

pub const REMOVE_EVENT: RemoveEvent = RemoveEvent {
    game: GAME,
    event: EVENT,
};

pub const REMOVE_GAME: RemoveGame = RemoveGame { game: GAME };

pub const HEARTBEAT: Heartbeat = Heartbeat { game: GAME };

#[derive(Debug, Clone)]
pub struct Engine {
    client: RawGameSenseClient,
}

impl Engine {
    pub async fn new() -> Result<Self> {
        let client = RawGameSenseClient::new()?;

        info!("{}", REGISTER_GAME.send(&client).await?);

        let x = BindGameEvent {
            game: GAME,
            event: EVENT,
            min_value: None,
            max_value: None,
            icon_id: None,
            value_optional: Some(true),
            handlers: vec![ScreenHandler {
                device: "screened-128x40",
                mode: "screen",
                zone: "one",
                datas: vec![Screen {
                    has_text: false,
                    image_data: vec![0u8; 640],
                }],
            }],
        }
        .send(&client)
        .await?;
        info!("{}", x);

        Ok(Self { client })
    }

    pub async fn heartbeat(&self) -> Result<()> {
        info!("{}", HEARTBEAT.send(&self.client).await?);
        Ok(())
    }
}

impl AsyncDevice for Engine {
    type ClearResult<'a> = impl Future<Output = Result<()>> + 'a;
    type DrawResult<'a> = impl Future<Output = Result<()>> + 'a;
    type ShutdownResult<'a> = impl Future<Output = Result<()>> + 'a;

    #[allow(clippy::needless_lifetimes)]
    fn draw<'this>(&'this mut self, display: &'this FrameBuffer) -> Self::DrawResult<'this> {
        async {
            let screen = display.framebuffer.as_buffer();

            let event = GameEvent {
                game: GAME,
                event: EVENT,
                data: FrameContainer {
                    frame: ScreenFrameData {
                        image_128x40: Some(<&[u8; 640]>::try_from(&screen[1..641])?),
                        ..Default::default()
                    },
                },
            };

            info!("{}", event.send(&self.client).await?);

            Ok(())
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn clear<'this>(&'this mut self) -> Self::ClearResult<'this> {
        async {
            let empty = FrameBuffer::new();
            self.draw(&empty).await?;
            Ok(())
        }
    }

    #[allow(clippy::needless_lifetimes)]
    fn shutdown<'this>(&'this mut self) -> Self::ShutdownResult<'this> {
        async {
            info!("{}", REMOVE_EVENT.send(&self.client).await?);
            info!("{}", REMOVE_GAME.send(&self.client).await?);
            Ok(())
        }
    }
}
