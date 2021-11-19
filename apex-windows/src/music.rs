use anyhow::{anyhow, Result};
use apex_music::{AsyncPlayer, Metadata as MetadataTrait, PlaybackStatus};
use std::future::Future;
use windows::Media::{
    Control,
    Control::{
        GlobalSystemMediaTransportControlsSession,
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionMediaProperties,
        GlobalSystemMediaTransportControlsSessionPlaybackInfo,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    },
};

#[derive(Debug, Clone, Default)]
struct Metadata {
    title: String,
    artists: String,
}

impl MetadataTrait for Metadata {
    fn title(&self) -> Result<String> {
        Ok(self.title.clone())
    }

    fn artists(&self) -> Result<String> {
        Ok(self.artists.clone())
    }

    fn length(&self) -> Result<i64> {
        Ok(0)
    }
}

struct Player {
    session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

impl Player {
    pub fn new() -> Result<Self> {
        let session_manager =
            Control::GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .map_err(|_| anyhow!("Windows"))?
                .get()
                .map_err(|_| anyhow!("Windows"))?;

        Ok(Self { session_manager })
    }

    pub fn current_session(&self) -> Result<GlobalSystemMediaTransportControlsSession> {
        self.session_manager
            .GetCurrentSession()
            .map_err(|e| anyhow!("Couldn't get current session: {}", e))
    }

    pub async fn media_properties(
        &self,
    ) -> Result<GlobalSystemMediaTransportControlsSessionMediaProperties> {
        let session = self.current_session()?;
        let x = session
            .TryGetMediaPropertiesAsync()
            .map_err(|e| anyhow!("Couldn't get media properties: {}", e))?
            .await;

        Ok(x)
    }
}
impl AsyncPlayer for Player {
    type Metadata = Metadata;

    type MetadataFuture<'b>
    where
        Self: 'b,
    = impl Future<Output = Result<Self::Metadata>> + 'b;
    type NameFuture<'b>
    where
        Self: 'b,
    = impl Future<Output = String> + 'b;
    type PlaybackStatusFuture<'b>
    where
        Self: 'b,
    = impl Future<Output = Result<PlaybackStatus>> + 'b;
    type PositionFuture<'b>
    where
        Self: 'b,
    = impl Future<Output = Result<i64>> + 'b;

    fn metadata<'this>(&'this self) -> Self::MetadataFuture<'this> {
        async {
            let session = self.media_properties().await?;
            session.Title()
        }
    }

    fn playback_status<'this>(&'this self) -> Self::PlaybackStatusFuture<'this> {
        async {
            let session = self.current_session();
            let session = match session {
                Ok(session) => session,
                Err(_) => return Ok(PlaybackStatus::Stopped),
            };

            let playback: GlobalSystemMediaTransportControlsSessionPlaybackInfo =
                session.GetPlaybackInfo().map_err(|_| anyhow!("Windows"))?;

            let status = playback.PlaybackStatus().map_err(|_| anyhow!("Windows"))?;

            Ok(match status {
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => {
                    PlaybackStatus::Playing
                }
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => {
                    PlaybackStatus::Paused
                }
                _ => PlaybackStatus::Stopped,
            })
        }
    }

    fn name<'this>(&'this self) -> Self::NameFuture<'this> {
        // There might be a Windows API to find the name of the player but the user most
        // likely will never see this anyway
        async { String::from("windows-api") }
    }

    fn position<'this>(&'this self) -> Self::PositionFuture<'this> {
        // TODO: Find the API for this?
        async { Ok(0) }
    }
}
