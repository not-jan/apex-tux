#![feature(type_alias_impl_trait, impl_trait_in_assoc_type)]
mod engine;
pub use engine::{Engine, HEARTBEAT, REMOVE_EVENT, REMOVE_GAME};
