#![feature(generic_associated_types, type_alias_impl_trait)]
mod player;
pub use player::{AsyncMetadata, AsyncPlayer, Metadata, PlaybackStatus, Player};
pub use player::Progress;
pub use player::PlayerEvent;