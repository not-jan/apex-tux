#![feature(type_alias_impl_trait)]
#![feature(impl_trait_in_assoc_type)]
mod player;
pub use player::{
    AsyncMetadata, AsyncPlayer, Metadata, PlaybackStatus, Player, PlayerEvent, Progress,
};
