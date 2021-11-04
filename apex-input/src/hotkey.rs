use crate::Command;
use anyhow::Result;
use tauri_hotkey::{Hotkey, HotkeyManager, Key, Modifier};
use tokio::sync::mpsc;

pub struct InputManager {
    _hkm: HotkeyManager,
}

impl InputManager {
    pub fn new(sender: mpsc::Sender<Command>) -> Result<Self> {
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
                    .blocking_send(Command::PreviousSource)
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
                    .blocking_send(Command::NextSource)
                    .expect("Failed to send command!");
            },
        )?;

        Ok(Self { _hkm: hkm })
    }
}
