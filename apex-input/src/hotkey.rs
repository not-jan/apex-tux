use crate::Command;
use anyhow::Result;
use global_hotkey::{GlobalHotKeyManager, hotkey::{HotKey, Modifiers, Code}, GlobalHotKeyEvent};
use tokio::sync::broadcast;

pub struct InputManager {
    _hkm: GlobalHotKeyManager,
}

impl InputManager {
    pub fn new(sender: broadcast::Sender<Command>) -> Result<Self> {
        let hkm = GlobalHotKeyManager::new().unwrap();

        let modifiers = Some(Modifiers::ALT | Modifiers::SHIFT);

        let hotkey_previous = HotKey::new (modifiers, Code::KeyA);
        let hotkey_next = HotKey::new (modifiers, Code::KeyD);

        hkm.register(hotkey_previous).unwrap();
        hkm.register(hotkey_next).unwrap();

        let hotkey_handler = move|event: GlobalHotKeyEvent| {
            if event.id == hotkey_previous.id() {
                sender
                    .send(Command::PreviousSource)
                    .expect("Failed to send command!");
            }else{
                sender
                    .send(Command::NextSource)
                    .expect("Failed to send command!");
            }
        };

        GlobalHotKeyEvent::set_event_handler(Some(hotkey_handler));

        Ok(Self { _hkm: hkm })
    }
}
