use anyhow::Result;
use std::future::Future;

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum PlaybackStatus {
    Stopped,
    Paused,
    Playing,
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    Seeked,
    Properties,
    Timer,
}

pub trait Metadata {
    fn title(&self) -> Result<String>;
    fn artists(&self) -> Result<String>;
    fn length(&self) -> Result<i64>;
}

pub trait Player {
    type Metadata: Metadata;
    fn metadata(&self) -> Result<Self::Metadata>;
    fn position(&self) -> Result<i64>;
    fn name(&self) -> String;
    fn playback_status(&self) -> Result<PlaybackStatus>;
}

pub struct Progress<T: Metadata + Sized> {
    pub metadata: T,
    pub position: i64,
    pub status: PlaybackStatus,
}

pub trait AsyncPlayer {
    type Metadata: Metadata;

    type MetadataFuture<'a>: Future<Output = Result<Self::Metadata>> + 'a
    where
        Self: 'a;

    type PlaybackStatusFuture<'a>: Future<Output = Result<PlaybackStatus>> + 'a
    where
        Self: 'a;

    type NameFuture<'a>: Future<Output = String> + 'a
    where
        Self: 'a;

    type PositionFuture<'a>: Future<Output = Result<i64>> + 'a
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn metadata<'this>(&'this self) -> Self::MetadataFuture<'this>;

    #[allow(clippy::needless_lifetimes)]
    fn playback_status<'this>(&'this self) -> Self::PlaybackStatusFuture<'this>;

    #[allow(clippy::needless_lifetimes)]
    fn name<'this>(&'this self) -> Self::NameFuture<'this>;

    #[allow(clippy::needless_lifetimes)]
    fn position<'this>(&'this self) -> Self::PositionFuture<'this>;
}

impl<T: Player + Sized> AsyncPlayer for T {
    type Metadata = <T as Player>::Metadata;

    type MetadataFuture<'a> = impl Future<Output = Result<Self::Metadata>> + 'a
    where
        T: 'a;
    type NameFuture<'a> = impl Future<Output = String>
    where
        T: 'a;
    type PlaybackStatusFuture<'a> = impl Future<Output = Result<PlaybackStatus>>
    where
        T: 'a;
    type PositionFuture<'a> = impl Future<Output = Result<i64>>
    where
        T: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn metadata<'this>(&'this self) -> Self::MetadataFuture<'this> {
        let metadata = <Self as Player>::metadata(self);
        async { metadata }
    }

    #[allow(clippy::needless_lifetimes)]
    fn playback_status<'this>(&'this self) -> Self::PlaybackStatusFuture<'this> {
        let status = <Self as Player>::playback_status(self);
        async { status }
    }

    #[allow(clippy::needless_lifetimes)]
    fn name<'this>(&'this self) -> Self::NameFuture<'this> {
        let name = <Self as Player>::name(self);
        async { name }
    }

    #[allow(clippy::needless_lifetimes)]
    fn position<'this>(&'this self) -> Self::PositionFuture<'this> {
        let position = <Self as Player>::position(self);
        async { position }
    }
}

pub trait AsyncMetadata {
    type TitleFuture<'a>: Future<Output = Result<String>> + 'a
    where
        Self: 'a;
    type ArtistsFuture<'a>: Future<Output = Result<String>> + 'a
    where
        Self: 'a;
    type LengthFuture<'a>: Future<Output = Result<i64>> + 'a
    where
        Self: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn title<'this>(&'this self) -> Self::TitleFuture<'this>;

    #[allow(clippy::needless_lifetimes)]
    fn artists<'this>(&'this self) -> Self::ArtistsFuture<'this>;

    #[allow(clippy::needless_lifetimes)]
    fn length<'this>(&'this self) -> Self::LengthFuture<'this>;
}

/// Blanket implementation for non-async Metadata sources
impl<T: Metadata + Sized> AsyncMetadata for T {
    type ArtistsFuture<'a> = impl Future<Output = Result<String>> + 'a
    where
        T: 'a;
    type LengthFuture<'a> = impl Future<Output = Result<i64>> + 'a
    where
        T: 'a;
    type TitleFuture<'a> = impl Future<Output = Result<String>> + 'a
    where
        T: 'a;

    #[allow(clippy::needless_lifetimes)]
    fn title<'this>(&'this self) -> Self::TitleFuture<'this> {
        let title = <Self as Metadata>::title(self);
        async { title }
    }

    #[allow(clippy::needless_lifetimes)]
    fn artists<'this>(&'this self) -> Self::ArtistsFuture<'this> {
        let artists = <Self as Metadata>::artists(self);
        async { artists }
    }

    #[allow(clippy::needless_lifetimes)]
    fn length<'this>(&'this self) -> Self::LengthFuture<'this> {
        let length = <Self as Metadata>::length(self);
        async { length }
    }
}
