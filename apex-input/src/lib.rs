#[cfg(feature = "hotkeys")]
mod hotkey;
mod input;
#[cfg(feature = "hotkeys")]
pub use hotkey::InputManager;
pub use input::Command;
