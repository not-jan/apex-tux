use anyhow::Result;
use apex_hardware::{Device, FrameBuffer};
use apex_input::Command;
use embedded_graphics::{geometry::Size, pixelcolor::BinaryColor, Drawable};
use embedded_graphics_simulator::{
    sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use std::{sync::mpsc, thread, thread::JoinHandle, time::Duration};

static WINDOW_TITLE: &str = concat!(
    env!("CARGO_PKG_NAME"),
    " v",
    env!("CARGO_PKG_VERSION"),
    " simulator"
);

pub struct Simulator {
    _handle: JoinHandle<Result<()>>,
    sender: mpsc::Sender<FrameBuffer>,
}

impl Simulator {
    pub fn connect(sender: tokio::sync::broadcast::Sender<Command>) -> Self {
        let (tx, rx) = mpsc::channel::<FrameBuffer>();
        let handle = thread::spawn(move || {
            let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(128, 40));

            let output_settings = OutputSettingsBuilder::new().scale(4).build();
            let mut window = Window::new(WINDOW_TITLE, &output_settings);

            'outer: loop {
                if let Ok(image) = rx.recv_timeout(Duration::from_millis(10)) {
                    image.draw(&mut display)?;
                }

                window.update(&display);

                for x in window.events() {
                    match x {
                        SimulatorEvent::KeyUp { keycode, .. } => {
                            if keycode == Keycode::Left {
                                sender.send(Command::PreviousSource)?;
                            } else if keycode == Keycode::Right {
                                sender.send(Command::NextSource)?;
                            }
                            Ok::<(), anyhow::Error>(())
                        }
                        SimulatorEvent::Quit => {
                            sender.send(Command::Shutdown)?;
                            break 'outer;
                        }
                        _ => Ok(()),
                    }?;
                }
            }

            Ok(())
        });

        Simulator {
            _handle: handle,
            sender: tx,
        }
    }
}

impl Device for Simulator {
    fn draw(&mut self, display: &FrameBuffer) -> Result<()> {
        self.sender.send(*display)?;
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        let new = FrameBuffer::new();
        self.draw(&new)?;
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
