#![feature(generic_associated_types, type_alias_impl_trait, async_iterator)]
mod generated;
mod player;
pub use player::{Metadata, Player, MPRIS2};
