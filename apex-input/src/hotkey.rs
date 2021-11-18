use crate::Command;
use anyhow::Result;
use tauri_hotkey::{Hotkey, HotkeyManager, Key, Modifier};
use tokio::sync::broadcast;

pub struct InputManager {
    _hkm: HotkeyManager,
}

impl InputManager {
    pub fn new(sender: broadcast::Sender<Command>) -> Result<Self> {
        let mut hkm = HotkeyManager::new();

        let modifiers = vec![Modifier::ALT, Modifier::SHIFT];

        let sender2 = sender.clone();

        hkm.register(
            Hotkey {
                modifiers: modifiers.clone(),
                keys: vec![Key::A],
            },
            move || {
                sender
                    .send(Command::PreviousSource)
                    .expect("Failed to send command!");
            },
        )?;
        hkm.register(
            Hotkey {
                modifiers,
                keys: vec![Key::D],
            },
            move || {
                sender2
                    .send(Command::NextSource)
                    .expect("Failed to send command!");
            },
        )?;

        Ok(Self { _hkm: hkm })
    }
}
