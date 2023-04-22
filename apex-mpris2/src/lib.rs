#![feature(type_alias_impl_trait, async_iterator)]
#![feature(impl_trait_in_assoc_type)]
mod generated;
mod player;
pub use player::{Metadata, Player, MPRIS2};
