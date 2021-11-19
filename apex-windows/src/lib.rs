#![feature(generic_associated_types, type_alias_impl_trait,async_stream)]
mod music;
pub use music::Player;
pub use music::Metadata;